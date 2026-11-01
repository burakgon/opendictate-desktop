//! The floating HUD shown while listening and working, drawn with AppKit and Core Animation.
//!
//! It is a borderless, non-activating panel: it can never take keyboard focus (so pasted text
//! always lands in the app you are typing in), it floats above full-screen apps and on every
//! Space, ignores the mouse, survives "Hide OpenDictate" (⌘H), and costs no web view.

use std::{cell::RefCell, ptr::NonNull, time::Instant};

use block2::RcBlock;
use objc2::{rc::Retained, runtime::AnyObject, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAnimatablePropertyContainer, NSAnimationContext, NSAppearance, NSAppearanceCustomization, NSAppearanceNameDarkAqua,
    NSBackingStoreType, NSColor, NSEvent, NSFont, NSFontWeightMedium, NSFontWeightRegular,
    NSFontWeightSemibold, NSImage, NSImageSymbolConfiguration, NSImageView, NSLineBreakMode, NSPanel, NSScreen,
    NSStatusWindowLevel, NSTextField, NSView,
    NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView,
    NSWindowAnimationBehavior, NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_core_foundation::{CGFloat, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{CGColor, CGPath};
use objc2_foundation::{NSNumber, NSString};
use objc2_quartz_core::{
    kCACornerCurveContinuous, kCALineCapRound, kCAMediaTimingFunctionEaseInEaseOut, kCAMediaTimingFunctionLinear,
    CABasicAnimation, CALayer, CAMediaTiming, CAMediaTimingFunction, CAShapeLayer, CATransaction,
};

use crate::{
    engine::{Mode, Status},
    settings::OverlayPosition,
};

const PANEL_W: CGFloat = 540.0;
const PANEL_H: CGFloat = 132.0;
/// Room around the pill for the window shadow.
const PAD: CGFloat = 24.0;
const PILL_H: CGFloat = 48.0;
const ICON: CGFloat = 32.0;
const BARS: usize = 26;
const BAR_W: CGFloat = 3.0;
const BAR_GAP: CGFloat = 3.0;
const WAVE_H: CGFloat = 28.0;
/// Distance between the pill and the Dock / menu bar.
const EDGE_MARGIN_BOTTOM: CGFloat = 36.0;
const EDGE_MARGIN_TOP: CGFloat = 18.0;

fn rgba(r: u8, g: u8, b: u8, a: CGFloat) -> Retained<NSColor> {
    NSColor::colorWithSRGBRed_green_blue_alpha(r as CGFloat / 255.0, g as CGFloat / 255.0, b as CGFloat / 255.0, a)
}

fn cg(color: &NSColor) -> Retained<CGColor> {
    color.CGColor()
}

fn rect(x: CGFloat, y: CGFloat, w: CGFloat, h: CGFloat) -> CGRect {
    CGRect::new(CGPoint::new(x, y), CGSize::new(w, h))
}

#[derive(Clone, Copy, PartialEq)]
enum Look {
    Dictation,
    Command,
    Done,
    Error,
    Neutral,
}

impl Look {
    fn accent(self) -> Retained<NSColor> {
        match self {
            Look::Dictation => rgba(56, 189, 248, 1.0),
            Look::Command => rgba(232, 121, 249, 1.0),
            Look::Done => rgba(110, 231, 183, 1.0),
            Look::Error => rgba(253, 164, 175, 1.0),
            Look::Neutral => rgba(255, 255, 255, 0.55),
        }
    }

    fn wash(self) -> Retained<NSColor> {
        match self {
            Look::Dictation => rgba(14, 165, 233, 0.18),
            Look::Command => rgba(217, 70, 239, 0.22),
            Look::Done => rgba(16, 185, 129, 0.22),
            Look::Error => rgba(244, 63, 94, 0.22),
            Look::Neutral => rgba(255, 255, 255, 0.07),
        }
    }

    fn bar(self) -> Retained<NSColor> {
        match self {
            Look::Command => rgba(240, 171, 252, 1.0),
            _ => rgba(125, 211, 252, 1.0),
        }
    }
}

fn label(mtm: MainThreadMarker, size: CGFloat, weight: CGFloat, color: &NSColor) -> Retained<NSTextField> {
    let l = NSTextField::labelWithString(&NSString::from_str(""), mtm);
    l.setFont(Some(&NSFont::systemFontOfSize_weight(size, weight)));
    l.setTextColor(Some(color));
    l.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
    l.setMaximumNumberOfLines(1);
    l
}

fn layer_of(view: &NSView) -> Retained<CALayer> {
    view.setWantsLayer(true);
    view.layer().expect("layer-backed view")
}

fn symbol(name: &str, size: CGFloat) -> Option<Retained<NSImage>> {
    // SAFETY: plain AppKit factory calls (macOS 11+).
    let weight = unsafe { NSFontWeightSemibold };
    let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(&NSString::from_str(name), None)?;
    let config = NSImageSymbolConfiguration::configurationWithPointSize_weight(size, weight);
    image.imageWithSymbolConfiguration(&config)
}

fn number(v: f64) -> Retained<NSNumber> {
    NSNumber::new_f64(v)
}

/// Adds a looping animation of `key_path` between two values.
fn pulse(layer: &CALayer, key_path: &str, from: f64, to: f64, duration: f64, autoreverses: bool) {
    let anim = CABasicAnimation::animationWithKeyPath(Some(&NSString::from_str(key_path)));
    let from = number(from);
    let to = number(to);
    let from: &AnyObject = &from;
    let to: &AnyObject = &to;
    // SAFETY: NSNumber values are valid for scalar key paths.
    unsafe {
        anim.setFromValue(Some(from));
        anim.setToValue(Some(to));
        anim.setTimingFunction(Some(&CAMediaTimingFunction::functionWithName(kCAMediaTimingFunctionEaseInEaseOut)));
    }
    anim.setDuration(duration);
    anim.setAutoreverses(autoreverses);
    anim.setRepeatCount(f32::INFINITY);
    layer.addAnimation_forKey(&anim, Some(&NSString::from_str(key_path)));
}

struct Hud {
    panel: Retained<NSPanel>,
    pill: Retained<NSVisualEffectView>,
    _tint: Retained<CALayer>,
    icon_bg: Retained<NSView>,
    icon: Retained<NSImageView>,
    dot: Retained<CALayer>,
    halo: Retained<CALayer>,
    /// Progress ring: a faint track and a spinning arc in the mode's colour.
    track: Retained<CAShapeLayer>,
    arc: Retained<CAShapeLayer>,
    wave: Retained<NSView>,
    bars: Vec<Retained<CALayer>>,
    levels: [CGFloat; BARS],
    title: Retained<NSTextField>,
    detail: Retained<NSTextField>,
    badge: Retained<NSView>,
    badge_text: Retained<NSTextField>,
    timer: Retained<NSTextField>,
    status: Option<Status>,
    session_started: Instant,
    session: u64,
    shown_secs: u64,
    visible: bool,
    /// Bumped on every show/hide so a fade-out that finishes late doesn't hide a newer pill.
    generation: u64,
    position: OverlayPosition,
    pill_frame: CGRect,
}

thread_local! {
    static HUD: RefCell<Option<Hud>> = const { RefCell::new(None) };
}

fn with_hud(f: impl FnOnce(&mut Hud) + Send + 'static) {
    super::on_main(move || {
        HUD.with(|h| {
            if let Some(hud) = h.borrow_mut().as_mut() {
                f(hud)
            }
        })
    });
}

/// Creates the (hidden) panel.
pub fn create(position: OverlayPosition) {
    super::on_main(move || {
        let Some(mtm) = MainThreadMarker::new() else { return };
        let hud = Hud::new(mtm, position);
        HUD.with(|h| *h.borrow_mut() = Some(hud));
    });
}

pub fn show(position: OverlayPosition) {
    with_hud(move |h| h.show(position));
}

pub fn hide() {
    with_hud(|h| h.hide());
}

pub fn update(status: &Status) {
    let status = status.clone();
    with_hud(move |h| h.set_status(status));
}

pub fn level(value: f32) {
    with_hud(move |h| h.push_level(value));
}

impl Hud {
    fn new(mtm: MainThreadMarker, position: OverlayPosition) -> Self {
        let style = NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel;
        let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
            NSPanel::alloc(mtm),
            rect(0.0, 0.0, PANEL_W, PANEL_H),
            style,
            NSBackingStoreType::Buffered,
            false,
        );
        // SAFETY: we keep our own reference; AppKit must not release the panel on close.
        unsafe { panel.setReleasedWhenClosed(false) };
        panel.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::Stationary
                | NSWindowCollectionBehavior::IgnoresCycle,
        );
        panel.setOpaque(false);
        panel.setBackgroundColor(Some(&NSColor::clearColor()));
        panel.setHasShadow(true);
        panel.setIgnoresMouseEvents(true);
        panel.setHidesOnDeactivate(false);
        panel.setCanHide(false);
        panel.setBecomesKeyOnlyIfNeeded(true);
        // Above full-screen apps and the Dock. (Set last: `setFloatingPanel` would reset it.)
        panel.setLevel(NSStatusWindowLevel);
        panel.setExcludedFromWindowsMenu(true);
        panel.setAnimationBehavior(NSWindowAnimationBehavior::None);
        // SAFETY: the appearance name is an AppKit constant.
        let dark = NSAppearance::appearanceNamed(unsafe { NSAppearanceNameDarkAqua });
        panel.setAppearance(dark.as_deref());
        panel.setAlphaValue(0.0);

        let root = NSView::initWithFrame(NSView::alloc(mtm), rect(0.0, 0.0, PANEL_W, PANEL_H));
        layer_of(&root);
        panel.setContentView(Some(&root));

        // The pill: native HUD material, darkened a little so the colours stay vivid.
        let pill = NSVisualEffectView::initWithFrame(NSVisualEffectView::alloc(mtm), rect(0.0, 0.0, 260.0, PILL_H));
        pill.setMaterial(NSVisualEffectMaterial::HUDWindow);
        pill.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
        pill.setState(NSVisualEffectState::Active);
        let pill_layer = layer_of(&pill);
        pill_layer.setCornerRadius(PILL_H / 2.0);
        // SAFETY: AppKit constant.
        pill_layer.setCornerCurve(unsafe { kCACornerCurveContinuous });
        pill_layer.setMasksToBounds(true);
        pill_layer.setBorderWidth(1.0);
        pill_layer.setBorderColor(Some(&cg(&rgba(255, 255, 255, 0.10))));
        // Covers the largest pill; the pill's mask clips it.
        let tint = CALayer::layer();
        tint.setFrame(rect(0.0, 0.0, PANEL_W, PANEL_H));
        tint.setBackgroundColor(Some(&cg(&rgba(10, 10, 18, 0.55))));
        pill_layer.addSublayer(&tint);
        root.addSubview(&pill);

        // Icon well (left).
        let icon_bg = NSView::initWithFrame(NSView::alloc(mtm), rect(8.0, (PILL_H - ICON) / 2.0, ICON, ICON));
        let icon_layer = layer_of(&icon_bg);
        icon_layer.setCornerRadius(ICON / 2.0);
        pill.addSubview(&icon_bg);

        let halo = CALayer::layer();
        halo.setFrame(rect(10.0, 10.0, 12.0, 12.0));
        halo.setCornerRadius(6.0);
        icon_layer.addSublayer(&halo);
        let dot = CALayer::layer();
        dot.setFrame(rect(10.0, 10.0, 12.0, 12.0));
        dot.setCornerRadius(6.0);
        dot.setShadowOpacity(0.9);
        dot.setShadowRadius(6.0);
        dot.setShadowOffset(CGSize::new(0.0, 0.0));
        icon_layer.addSublayer(&dot);

        let icon = NSImageView::initWithFrame(NSImageView::alloc(mtm), rect(7.0, 7.0, 18.0, 18.0));
        icon_bg.addSubview(&icon);

        let ring = |width: CGFloat| {
            let layer = CAShapeLayer::layer();
            layer.setFrame(rect(7.0, 7.0, 18.0, 18.0));
            // SAFETY: a null transform is allowed.
            let path = unsafe { CGPath::with_ellipse_in_rect(rect(1.25, 1.25, 15.5, 15.5), std::ptr::null()) };
            layer.setPath(Some(&path));
            layer.setFillColor(None);
            layer.setLineWidth(width);
            // SAFETY: Core Animation constant.
            layer.setLineCap(unsafe { kCALineCapRound });
            layer.setHidden(true);
            icon_layer.addSublayer(&layer);
            layer
        };
        let track = ring(2.5);
        track.setStrokeColor(Some(&cg(&rgba(255, 255, 255, 0.12))));
        let arc = ring(2.5);
        arc.setStrokeEnd(0.3);

        // Waveform.
        let wave = NSView::initWithFrame(NSView::alloc(mtm), rect(52.0, (PILL_H - WAVE_H) / 2.0, wave_width(), WAVE_H));
        let wave_layer = layer_of(&wave);
        let mut bars = Vec::with_capacity(BARS);
        for i in 0..BARS {
            let bar = CALayer::layer();
            bar.setCornerRadius(BAR_W / 2.0);
            bar.setFrame(rect(i as CGFloat * (BAR_W + BAR_GAP), (WAVE_H - 3.0) / 2.0, BAR_W, 3.0));
            wave_layer.addSublayer(&bar);
            bars.push(bar);
        }
        pill.addSubview(&wave);

        // SAFETY: AppKit font weight constants.
        let (medium, regular) = unsafe { (NSFontWeightMedium, NSFontWeightRegular) };
        let title = label(mtm, 13.5, medium, &rgba(255, 255, 255, 0.9));
        pill.addSubview(&title);
        let detail = label(mtm, 11.0, regular, &rgba(255, 255, 255, 0.5));
        pill.addSubview(&detail);

        let badge = NSView::initWithFrame(NSView::alloc(mtm), rect(0.0, 0.0, 10.0, 18.0));
        let badge_layer = layer_of(&badge);
        badge_layer.setCornerRadius(9.0);
        let badge_text = label(mtm, 11.0, unsafe { NSFontWeightSemibold }, &rgba(255, 255, 255, 0.6));
        badge.addSubview(&badge_text);
        pill.addSubview(&badge);

        let timer = label(mtm, 12.0, regular, &rgba(255, 255, 255, 0.5));
        timer.setFont(Some(&NSFont::monospacedDigitSystemFontOfSize_weight(12.0, regular)));
        pill.addSubview(&timer);

        let hud = Hud {
            panel,
            pill,
            _tint: tint,
            icon_bg,
            icon,
            dot,
            halo,
            track,
            arc,
            wave,
            bars,
            levels: [0.0; BARS],
            title,
            detail,
            badge,
            badge_text,
            timer,
            status: None,
            session_started: Instant::now(),
            session: 0,
            shown_secs: u64::MAX,
            visible: false,
            generation: 0,
            position,
            pill_frame: rect(0.0, 0.0, 0.0, 0.0),
        };
        hud.place();
        hud
    }

    /// Puts the panel at the bottom (or top) centre of the screen the mouse is on.
    fn place(&self) {
        let Some(mtm) = MainThreadMarker::new() else { return };
        let mouse = NSEvent::mouseLocation();
        let screens = NSScreen::screens(mtm);
        let inside = |s: &NSScreen| {
            let f = s.frame();
            mouse.x >= f.origin.x && mouse.x <= f.origin.x + f.size.width && mouse.y >= f.origin.y && mouse.y <= f.origin.y + f.size.height
        };
        let screen = screens.iter().find(|s| inside(s)).or_else(|| NSScreen::mainScreen(mtm));
        let Some(screen) = screen else { return };
        let vf = screen.visibleFrame();
        let x = (vf.origin.x + (vf.size.width - PANEL_W) / 2.0).round();
        let y = match self.position {
            OverlayPosition::Bottom => vf.origin.y + EDGE_MARGIN_BOTTOM - PAD,
            OverlayPosition::Top => vf.origin.y + vf.size.height - EDGE_MARGIN_TOP - PANEL_H + PAD,
        };
        self.panel.setFrame_display(rect(x, y.round(), PANEL_W, PANEL_H), false);
    }

    fn show(&mut self, position: OverlayPosition) {
        self.generation += 1;
        if self.visible && position == self.position {
            return;
        }
        let was_visible = self.visible;
        self.position = position;
        self.visible = true;
        if !was_visible {
            self.place();
            self.layout(false);
            self.panel.orderFrontRegardless();
            let panel = self.panel.clone();
            let changes = RcBlock::new(move |ctx: NonNull<NSAnimationContext>| {
                // SAFETY: AppKit passes a valid context for the duration of the block.
                unsafe { ctx.as_ref() }.setDuration(0.16);
                panel.animator().setAlphaValue(1.0);
            });
            NSAnimationContext::runAnimationGroup(&changes);
        } else {
            self.place();
        }
    }

    fn hide(&mut self) {
        if !self.visible {
            return;
        }
        self.visible = false;
        self.generation += 1;
        let generation = self.generation;
        let panel = self.panel.clone();
        let changes = RcBlock::new(move |ctx: NonNull<NSAnimationContext>| {
            // SAFETY: valid for the duration of the block.
            unsafe { ctx.as_ref() }.setDuration(0.22);
            panel.animator().setAlphaValue(0.0);
        });
        let done = RcBlock::new(move || {
            HUD.with(|h| {
                if let Ok(mut h) = h.try_borrow_mut() {
                    if let Some(hud) = h.as_mut() {
                        if hud.generation == generation && !hud.visible {
                            hud.panel.orderOut(None);
                            hud.stop_animations();
                        }
                    }
                }
            });
        });
        NSAnimationContext::runAnimationGroup_completionHandler(&changes, Some(&done));
    }

    fn stop_animations(&self) {
        self.dot.removeAllAnimations();
        self.halo.removeAllAnimations();
        self.title.layer().inspect(|l| l.removeAllAnimations());
        self.arc.removeAllAnimations();
    }

    fn set_status(&mut self, status: Status) {
        if status.phase == "recording" && status.session != self.session {
            self.session = status.session;
            self.session_started = Instant::now();
            self.shown_secs = u64::MAX;
            self.levels = [0.0; BARS];
        }
        self.status = Some(status);
        self.layout(self.visible);
    }

    fn push_level(&mut self, level: f32) {
        if self.status.as_ref().map(|s| s.phase) != Some("recording") {
            return;
        }
        let v = ((level.max(0.0) as CGFloat).sqrt() * 3.4).min(1.0);
        self.levels.rotate_left(1);
        self.levels[BARS - 1] = v;
        CATransaction::begin();
        CATransaction::setAnimationDuration(0.08);
        for (i, bar) in self.bars.iter().enumerate() {
            let v = self.levels[i];
            // Taper the ends so the wave looks centred.
            let taper = (((i + 1) as CGFloat / (BARS + 1) as CGFloat) * std::f64::consts::PI).sin() * 0.55 + 0.45;
            let h = (v * WAVE_H * taper).max(3.0);
            bar.setFrame(rect(i as CGFloat * (BAR_W + BAR_GAP), ((WAVE_H - h) / 2.0).round(), BAR_W, h.round()));
            bar.setOpacity((0.45 + v * 0.55) as f32);
        }
        CATransaction::commit();
        self.update_timer();
    }

    fn update_timer(&mut self) {
        let secs = self.session_started.elapsed().as_secs();
        if secs != self.shown_secs {
            self.shown_secs = secs;
            self.timer.setStringValue(&NSString::from_str(&format!("{}:{:02}", secs / 60, secs % 60)));
        }
    }

    fn look(&self) -> Look {
        let Some(s) = &self.status else { return Look::Neutral };
        let command = matches!(s.mode, Mode::Command | Mode::Action);
        match s.phase {
            "recording" | "processing" if command => Look::Command,
            "recording" | "processing" => Look::Dictation,
            "done" => Look::Done,
            "error" => Look::Error,
            _ => Look::Neutral,
        }
    }

    /// Fills in the content for the current status and sizes the pill around it.
    fn layout(&mut self, animate: bool) {
        let Some(status) = self.status.clone() else { return };
        let look = self.look();
        let phase = status.phase;
        let recording = phase == "recording";
        let processing = phase == "processing";
        let error = phase == "error";

        // Icon well.
        let icon_layer = layer_of(&self.icon_bg);
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        let well = if processing { Look::Neutral.wash() } else { look.wash() };
        icon_layer.setBackgroundColor(Some(&cg(&well)));
        let show_dot = recording && look == Look::Dictation;
        self.dot.setHidden(!show_dot);
        self.halo.setHidden(!show_dot);
        if show_dot {
            let c = cg(&look.accent());
            self.dot.setBackgroundColor(Some(&c));
            self.dot.setShadowColor(Some(&c));
            self.halo.setBackgroundColor(Some(&cg(&rgba(56, 189, 248, 0.55))));
        }
        for bar in &self.bars {
            bar.setBackgroundColor(Some(&cg(&look.bar())));
        }
        CATransaction::commit();
        if show_dot && self.dot.animationKeys().is_none() {
            pulse(&self.dot, "opacity", 1.0, 0.55, 0.7, true);
            pulse(&self.halo, "transform.scale", 1.0, 2.4, 1.1, false);
            pulse(&self.halo, "opacity", 0.8, 0.0, 1.1, false);
        } else if !show_dot {
            self.dot.removeAllAnimations();
            self.halo.removeAllAnimations();
        }

        let symbol_name = match (phase, look) {
            ("recording", Look::Command) => Some("wand.and.stars"),
            ("done", _) => Some("checkmark"),
            ("error", _) => Some("exclamationmark"),
            ("cancelled", _) | ("idle", _) => Some("xmark"),
            _ => None,
        };
        match symbol_name.and_then(|n| symbol(n, if n == "wand.and.stars" { 14.0 } else { 13.0 })) {
            Some(img) => {
                self.icon.setImage(Some(&img));
                self.icon.setContentTintColor(Some(&look.accent()));
                self.icon.setHidden(false);
            }
            None => self.icon.setHidden(true),
        }
        self.track.setHidden(!processing);
        self.arc.setHidden(!processing);
        if processing {
            self.arc.setStrokeColor(Some(&cg(&look.accent())));
            if self.arc.animationKeys().is_none() {
                spin(&self.arc);
            }
        } else {
            self.arc.removeAllAnimations();
        }

        // Body: waveform while recording, text otherwise.
        self.wave.setHidden(!recording);
        let text = match phase {
            "processing" => format!("{}…", status.message.as_deref().unwrap_or("Working")),
            "done" => status.preview.clone().filter(|p| !p.is_empty()).or(status.message.clone()).unwrap_or_default(),
            "error" => status.message.clone().unwrap_or_else(|| "Something went wrong".into()),
            _ => status.message.clone().unwrap_or_else(|| "Cancelled".into()),
        }
        .replace('\n', " ");
        self.title.setHidden(recording);
        self.title.setStringValue(&NSString::from_str(&text));
        let title_color = match phase {
            "error" => rgba(255, 228, 230, 0.92),
            "cancelled" | "idle" => rgba(255, 255, 255, 0.55),
            _ => rgba(255, 255, 255, 0.9),
        };
        self.title.setTextColor(Some(&title_color));
        if error {
            self.title.setLineBreakMode(NSLineBreakMode::ByWordWrapping);
            self.title.setMaximumNumberOfLines(2);
            self.title.setFont(Some(&NSFont::systemFontOfSize_weight(12.5, unsafe { NSFontWeightRegular })));
        } else {
            self.title.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
            self.title.setMaximumNumberOfLines(1);
            self.title.setFont(Some(&NSFont::systemFontOfSize_weight(13.5, unsafe { NSFontWeightMedium })));
        }
        let title_layer = layer_of(&self.title);
        if processing {
            if title_layer.animationKeys().is_none() {
                pulse(&title_layer, "opacity", 1.0, 0.5, 0.9, true);
            }
        } else {
            title_layer.removeAllAnimations();
        }
        let hint = error && status.retryable;
        self.detail.setHidden(!hint);
        if hint {
            self.detail.setStringValue(&NSString::from_str("Your recording was kept — retry it from the menu bar."));
        }

        // Tail: a badge and/or the timer.
        let badge = match phase {
            "recording" if look == Look::Command => Some(("Command".to_string(), look)),
            "processing" => status.selection_chars.map(|n| (format!("{n} chars selected"), Look::Neutral)),
            "done" if status.message.as_deref() == Some("Copied to clipboard") => Some(("copied".to_string(), Look::Neutral)),
            _ => None,
        };
        self.timer.setHidden(!recording);
        if recording {
            self.update_timer();
        }

        // Measure.
        let wide = matches!(phase, "done" | "error") || (processing && status.selection_chars.is_some());
        let max_w: CGFloat = if wide || (recording && look == Look::Command) { 456.0 } else { 340.0 };
        let left: CGFloat = 8.0 + ICON + 12.0;
        let right_pad: CGFloat = 16.0;
        let mut tail_w: CGFloat = 0.0;
        let badge_size = badge.as_ref().map(|(t, l)| {
            self.badge_text.setStringValue(&NSString::from_str(t));
            let color = if *l == Look::Neutral { rgba(255, 255, 255, 0.6) } else { l.accent() };
            self.badge_text.setTextColor(Some(&color));
            layer_of(&self.badge).setBackgroundColor(Some(&cg(&l.wash())));
            let s = self.badge_text.fittingSize();
            CGSize::new((s.width + 16.0).ceil(), 18.0)
        });
        self.badge.setHidden(badge_size.is_none());
        if let Some(s) = badge_size {
            tail_w += s.width;
        }
        let timer_w: CGFloat = if recording { self.timer.fittingSize().width.ceil().max(28.0) } else { 0.0 };
        if recording {
            tail_w += if tail_w > 0.0 { 8.0 } else { 0.0 } + timer_w;
        }
        let gap_before_tail = if tail_w > 0.0 { 12.0 } else { 0.0 };
        let body_max = max_w - left - right_pad - gap_before_tail - tail_w;
        let (body_w, body_h) = if recording {
            (wave_width(), WAVE_H)
        } else {
            self.title.setPreferredMaxLayoutWidth(body_max);
            let s = self.title.fittingSize();
            (s.width.ceil().min(body_max), s.height.ceil())
        };
        let detail_h = if hint { 15.0 } else { 0.0 };
        let content_h = body_h + detail_h;
        let pill_h = if error { (content_h + 18.0).max(PILL_H) } else { PILL_H };
        let pill_w = (left + body_w + gap_before_tail + tail_w + right_pad).clamp(200.0, max_w).round();

        // Place everything inside the pill (AppKit coordinates: origin bottom-left).
        let mid = pill_h / 2.0;
        self.icon_bg.setFrame(rect(8.0, (mid - ICON / 2.0).round(), ICON, ICON));
        self.wave.setFrame(rect(left, (mid - WAVE_H / 2.0).round(), wave_width(), WAVE_H));
        let text_top = mid + content_h / 2.0;
        self.title.setFrame(rect(left, (text_top - body_h).round(), body_w, body_h));
        self.detail.setFrame(rect(left, (text_top - body_h - detail_h).round(), body_max, detail_h));
        let mut x = pill_w - right_pad;
        if recording {
            x -= timer_w;
            self.timer.setFrame(rect(x, (mid - 8.0).round(), timer_w, 16.0));
            x -= 8.0;
        }
        if let Some(s) = badge_size {
            x -= s.width;
            self.badge.setFrame(rect(x, (mid - s.height / 2.0).round(), s.width, s.height));
            let ts = self.badge_text.fittingSize();
            self.badge_text.setFrame(rect(8.0, ((s.height - ts.height) / 2.0).round(), s.width - 16.0, ts.height.ceil()));
        }

        let pill_x = ((PANEL_W - pill_w) / 2.0).round();
        let pill_y = match self.position {
            OverlayPosition::Bottom => PAD,
            OverlayPosition::Top => PANEL_H - PAD - pill_h,
        };
        let frame = rect(pill_x, pill_y, pill_w, pill_h);
        let radius = if error { 18.0 } else { pill_h / 2.0 };
        if animate && frame != self.pill_frame {
            let pill = self.pill.clone();
            let changes = RcBlock::new(move |ctx: NonNull<NSAnimationContext>| {
                // SAFETY: valid for the duration of the block.
                let ctx = unsafe { ctx.as_ref() };
                ctx.setDuration(0.2);
                ctx.setAllowsImplicitAnimation(true);
                pill.animator().setFrame(frame);
            });
            let panel = self.panel.clone();
            let done = RcBlock::new(move || panel.invalidateShadow());
            NSAnimationContext::runAnimationGroup_completionHandler(&changes, Some(&done));
        } else {
            self.pill.setFrame(frame);
            self.panel.invalidateShadow();
        }
        layer_of(&self.pill).setCornerRadius(radius);
        self.pill_frame = frame;
    }
}

/// Rotates a layer forever around its centre.
fn spin(layer: &CALayer) {
    let anim = CABasicAnimation::animationWithKeyPath(Some(&NSString::from_str("transform.rotation.z")));
    let from = number(0.0);
    let to = number(std::f64::consts::TAU);
    let from: &AnyObject = &from;
    let to: &AnyObject = &to;
    // SAFETY: NSNumber values are valid for the rotation key path.
    unsafe {
        anim.setFromValue(Some(from));
        anim.setToValue(Some(to));
        anim.setTimingFunction(Some(&CAMediaTimingFunction::functionWithName(kCAMediaTimingFunctionLinear)));
    }
    anim.setDuration(0.8);
    anim.setRepeatCount(f32::INFINITY);
    layer.addAnimation_forKey(&anim, Some(&NSString::from_str("spin")));
}

fn wave_width() -> CGFloat {
    BARS as CGFloat * BAR_W + (BARS - 1) as CGFloat * BAR_GAP
}
