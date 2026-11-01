//! macOS shortcuts that the system hotkey API (Carbon `RegisterEventHotKey`, used for regular
//! combinations) can't express: a single modifier key such as Right ⌘, Right ⌥ or fn (🌐), mouse
//! buttons, and combinations that turned out to be taken.
//!
//! They are read through a Quartz event tap, which macOS only allows with Accessibility (or Input
//! Monitoring) permission. Until that is granted a watcher checks once a second and starts the
//! tap as soon as it is allowed, so the shortcut starts working without a restart. Modifier keys
//! are only observed, never swallowed; a regular key that completes a shortcut is swallowed so it
//! doesn't also type into the focused app.

use std::{
    collections::HashSet,
    ffi::c_void,
    ptr,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc,
    },
    thread,
    time::Duration,
};

use parking_lot::Mutex;

use super::{Binding, EventTx, HotkeyEvent, HotkeyStatus};
use crate::macos::{
    ffi::{self, CGEventFlags, CGEventRef, CGEventTapProxy, CGEventType},
    keys::EVENT_MARKER,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    /// A virtual key code (`kVK_*`).
    Code(u16),
    /// A mouse button number (0 left, 1 right, 2 middle, 3 back, 4 forward).
    Mouse(i64),
}

const KVK_RIGHT_CMD: u16 = 54;
const KVK_CMD: u16 = 55;
const KVK_SHIFT: u16 = 56;
const KVK_OPTION: u16 = 58;
const KVK_CONTROL: u16 = 59;
const KVK_RIGHT_SHIFT: u16 = 60;
const KVK_RIGHT_OPTION: u16 = 61;
const KVK_RIGHT_CONTROL: u16 = 62;
const KVK_FUNCTION: u16 = 63;

/// 0 control, 1 shift, 2 option, 3 command, 4 fn.
fn mod_class(key: Key) -> Option<u8> {
    match key {
        Key::Code(KVK_CONTROL | KVK_RIGHT_CONTROL) => Some(0),
        Key::Code(KVK_SHIFT | KVK_RIGHT_SHIFT) => Some(1),
        Key::Code(KVK_OPTION | KVK_RIGHT_OPTION) => Some(2),
        Key::Code(KVK_CMD | KVK_RIGHT_CMD) => Some(3),
        Key::Code(KVK_FUNCTION) => Some(4),
        _ => None,
    }
}

/// Virtual key codes for a canonical token (see `hotkeys::parse`). Letters and digits refer to
/// physical key positions, like the tokens recorded in the settings window.
pub fn token_keys(token: &str) -> Option<Vec<Key>> {
    let one = |c: u16| Some(vec![Key::Code(c)]);
    match token {
        "Ctrl" => Some(vec![Key::Code(KVK_CONTROL), Key::Code(KVK_RIGHT_CONTROL)]),
        "Shift" => Some(vec![Key::Code(KVK_SHIFT), Key::Code(KVK_RIGHT_SHIFT)]),
        "Alt" => Some(vec![Key::Code(KVK_OPTION), Key::Code(KVK_RIGHT_OPTION)]),
        "Super" => Some(vec![Key::Code(KVK_CMD), Key::Code(KVK_RIGHT_CMD)]),
        "LeftCtrl" => one(KVK_CONTROL),
        "RightCtrl" => one(KVK_RIGHT_CONTROL),
        "LeftShift" => one(KVK_SHIFT),
        "RightShift" => one(KVK_RIGHT_SHIFT),
        "LeftAlt" => one(KVK_OPTION),
        "RightAlt" => one(KVK_RIGHT_OPTION),
        "LeftSuper" => one(KVK_CMD),
        "RightSuper" => one(KVK_RIGHT_CMD),
        "Fn" => one(KVK_FUNCTION),
        "MouseMiddle" => Some(vec![Key::Mouse(2)]),
        "Mouse4" => Some(vec![Key::Mouse(3)]),
        "Mouse5" => Some(vec![Key::Mouse(4)]),
        "Space" => one(49),
        "Enter" => one(36),
        "Tab" => one(48),
        "Escape" => one(53),
        "Backspace" => one(51),
        "Delete" => one(117),
        "Home" => one(115),
        "End" => one(119),
        "PageUp" => one(116),
        "PageDown" => one(121),
        "Up" => one(126),
        "Down" => one(125),
        "Left" => one(123),
        "Right" => one(124),
        "`" => one(50),
        "-" => one(27),
        "=" => one(24),
        "[" => one(33),
        "]" => one(30),
        "\\" => one(42),
        ";" => one(41),
        "'" => one(39),
        "," => one(43),
        "." => one(47),
        "/" => one(44),
        "NumpadEnter" => one(76),
        "Numpad+" => one(69),
        "Numpad-" => one(78),
        "Numpad*" => one(67),
        "Numpad/" => one(75),
        "Numpad." => one(65),
        t if t.len() == 1 => {
            const LETTERS: [u16; 26] =
                [0, 11, 8, 2, 14, 3, 5, 4, 34, 38, 40, 37, 46, 45, 31, 35, 12, 15, 1, 17, 32, 9, 13, 7, 16, 6];
            const DIGITS: [u16; 10] = [29, 18, 19, 20, 21, 23, 22, 26, 28, 25];
            let c = t.chars().next()?.to_ascii_uppercase();
            match c {
                'A'..='Z' => one(LETTERS[(c as u8 - b'A') as usize]),
                '0'..='9' => one(DIGITS[(c as u8 - b'0') as usize]),
                _ => None,
            }
        }
        t if t.starts_with("Numpad") => {
            const PAD: [u16; 10] = [82, 83, 84, 85, 86, 87, 88, 89, 91, 92];
            let d: usize = t[6..].parse().ok()?;
            PAD.get(d).map(|c| vec![Key::Code(*c)])
        }
        t if t.starts_with('F') && t.len() <= 3 => {
            const F: [u16; 20] = [122, 120, 99, 118, 96, 97, 98, 100, 101, 109, 103, 111, 105, 107, 113, 106, 64, 79, 80, 90];
            let n: usize = t[1..].parse().ok()?;
            (1..=20).contains(&n).then(|| vec![Key::Code(F[n - 1])])
        }
        _ => None,
    }
}

/// Shortcuts that need the event tap rather than a Carbon hotkey.
pub fn needs_tap(binding: &Binding) -> bool {
    binding.modifier_only() || binding.tokens.iter().any(|t| t == "Fn" || t.starts_with("Mouse"))
}

#[derive(Debug)]
struct TapBinding {
    binding: Binding,
    /// Each token is satisfied by any one of these keys.
    tokens: Vec<Vec<Key>>,
    mod_classes: HashSet<u8>,
    modifier_only: bool,
    active: bool,
    suppressed: bool,
}

impl TapBinding {
    fn compile(binding: &Binding) -> Option<Self> {
        let mut tokens = Vec::new();
        let mut mod_classes = HashSet::new();
        for t in &binding.tokens {
            let keys = token_keys(t)?;
            if let Some(c) = mod_class(keys[0]) {
                mod_classes.insert(c);
            }
            tokens.push(keys);
        }
        let modifier_only = tokens.iter().all(|alts| mod_class(alts[0]).is_some());
        Some(Self { binding: binding.clone(), tokens, mod_classes, modifier_only, active: false, suppressed: false })
    }

    fn involves(&self, key: Key) -> bool {
        self.tokens.iter().any(|alts| alts.contains(&key))
    }

    fn satisfied(&self, pressed: &HashSet<Key>) -> bool {
        if !self.tokens.iter().all(|alts| alts.iter().any(|k| pressed.contains(k))) {
            return false;
        }
        // Extra modifiers (⇧ while the shortcut is Right ⌘) don't match.
        pressed.iter().filter_map(|k| mod_class(*k)).all(|c| self.mod_classes.contains(&c))
    }
}

#[derive(Default)]
struct State {
    bindings: Vec<TapBinding>,
    pressed: HashSet<Key>,
    /// Keys whose key-up we swallow because their key-down completed a shortcut.
    swallowing: HashSet<Key>,
    tx: Option<EventTx>,
}

impl State {
    /// Forgets modifiers the event says aren't held (we may have missed their release, e.g.
    /// while secure keyboard entry was on).
    fn resync(&mut self, flags: CGEventFlags) {
        let classes = [
            (ffi::kCGEventFlagMaskControl, [KVK_CONTROL, KVK_RIGHT_CONTROL]),
            (ffi::kCGEventFlagMaskShift, [KVK_SHIFT, KVK_RIGHT_SHIFT]),
            (ffi::kCGEventFlagMaskAlternate, [KVK_OPTION, KVK_RIGHT_OPTION]),
            (ffi::kCGEventFlagMaskCommand, [KVK_CMD, KVK_RIGHT_CMD]),
        ];
        for (mask, codes) in classes {
            if flags & mask == 0 {
                for c in codes {
                    self.pressed.remove(&Key::Code(c));
                }
            }
        }
    }

    fn any_active(&self) -> bool {
        self.bindings.iter().any(|b| b.active)
    }

    /// Modifier keys we think are down but the system says are up (a release we never saw).
    fn stale_modifiers(&self) -> Vec<Key> {
        // SAFETY: plain query. The combined session state also covers keys sent by remote-control
        // and remapping tools, not just the hardware.
        let flags = unsafe { ffi::CGEventSourceFlagsState(ffi::kCGEventSourceStateCombinedSessionState) };
        self.pressed
            .iter()
            .filter(|k| match k {
                Key::Code(c) => modifier_down(*c, flags) == Some(false),
                Key::Mouse(_) => false,
            })
            .copied()
            .collect()
    }

    /// Releases everything, e.g. after the tap was paused by the system.
    fn reset(&mut self) {
        for b in self.bindings.iter_mut() {
            if b.active {
                b.active = false;
                if let Some(tx) = &self.tx {
                    let _ = tx.send(HotkeyEvent::Released(b.binding.slot));
                }
            }
            b.suppressed = false;
        }
        self.pressed.clear();
        self.swallowing.clear();
    }

    /// Feeds one input event through the shortcut matcher. Returns true to swallow the event.
    /// `transient` events (plain clicks) only interrupt modifier shortcuts.
    fn handle(&mut self, key: Key, down: bool, repeat: bool, transient: bool, paused: bool) -> bool {
        if repeat {
            return down && self.swallowing.contains(&key);
        }
        if !transient {
            if down {
                self.pressed.insert(key);
            } else {
                self.pressed.remove(&key);
            }
        }
        let mut swallow = !down && self.swallowing.remove(&key);
        if paused || self.tx.is_none() {
            return swallow;
        }
        let State { bindings, pressed, swallowing, tx } = self;
        let mut events = Vec::new();
        for b in bindings.iter_mut() {
            if b.suppressed {
                if !b.tokens.iter().any(|alts| alts.iter().any(|k| pressed.contains(k))) {
                    b.suppressed = false;
                }
                continue;
            }
            let sat = b.satisfied(pressed);
            if b.active {
                if down && !b.involves(key) && b.modifier_only {
                    // e.g. Right ⌘ held and then C pressed: an ordinary ⌘C.
                    b.active = false;
                    b.suppressed = true;
                    events.push(HotkeyEvent::Interrupted(b.binding.slot));
                } else if !sat {
                    b.active = false;
                    events.push(HotkeyEvent::Released(b.binding.slot));
                }
            } else if down && sat && b.involves(key) {
                b.active = true;
                events.push(HotkeyEvent::Pressed(b.binding.slot));
                if mod_class(key).is_none() && !transient {
                    swallow = true;
                    swallowing.insert(key);
                }
            }
        }
        if let Some(tx) = tx {
            for ev in events {
                let _ = tx.send(ev);
            }
        }
        swallow
    }
}

/// Whether a modifier key went down or up, from a flags-changed event.
fn modifier_down(code: u16, flags: CGEventFlags) -> Option<bool> {
    use ffi::*;
    let (bit, class, class_bits) = match code {
        KVK_RIGHT_CMD => (NX_DEVICERCMDKEYMASK, kCGEventFlagMaskCommand, NX_DEVICELCMDKEYMASK | NX_DEVICERCMDKEYMASK),
        KVK_CMD => (NX_DEVICELCMDKEYMASK, kCGEventFlagMaskCommand, NX_DEVICELCMDKEYMASK | NX_DEVICERCMDKEYMASK),
        KVK_SHIFT => (NX_DEVICELSHIFTKEYMASK, kCGEventFlagMaskShift, NX_DEVICELSHIFTKEYMASK | NX_DEVICERSHIFTKEYMASK),
        KVK_RIGHT_SHIFT => (NX_DEVICERSHIFTKEYMASK, kCGEventFlagMaskShift, NX_DEVICELSHIFTKEYMASK | NX_DEVICERSHIFTKEYMASK),
        KVK_OPTION => (NX_DEVICELALTKEYMASK, kCGEventFlagMaskAlternate, NX_DEVICELALTKEYMASK | NX_DEVICERALTKEYMASK),
        KVK_RIGHT_OPTION => (NX_DEVICERALTKEYMASK, kCGEventFlagMaskAlternate, NX_DEVICELALTKEYMASK | NX_DEVICERALTKEYMASK),
        KVK_CONTROL => (NX_DEVICELCTLKEYMASK, kCGEventFlagMaskControl, NX_DEVICELCTLKEYMASK | NX_DEVICERCTLKEYMASK),
        KVK_RIGHT_CONTROL => (NX_DEVICERCTLKEYMASK, kCGEventFlagMaskControl, NX_DEVICELCTLKEYMASK | NX_DEVICERCTLKEYMASK),
        KVK_FUNCTION => return Some(flags & kCGEventFlagMaskSecondaryFn != 0),
        _ => return None,
    };
    // Some keyboards don't report which side is pressed; fall back to the generic flag.
    Some(if flags & class_bits != 0 { flags & bit != 0 } else { flags & class != 0 })
}

/// Lives as long as the tap's run loop; passed to the callback as `user_info`.
struct TapContext {
    state: Arc<Mutex<State>>,
    tap: Mutex<ffi::CFMachPortRef>,
    can_swallow: bool,
}

extern "C" fn tap_callback(_proxy: CGEventTapProxy, kind: CGEventType, event: CGEventRef, user_info: *mut c_void) -> CGEventRef {
    // SAFETY: `user_info` is the `TapContext` owned by the tap thread, alive while the tap runs.
    let ctx = unsafe { &*(user_info as *const TapContext) };
    if kind == ffi::kCGEventTapDisabledByTimeout || kind == ffi::kCGEventTapDisabledByUserInput {
        log::info!("hotkeys: the event tap was paused by macOS; resuming");
        let tap = *ctx.tap.lock();
        if !tap.is_null() {
            // SAFETY: the tap is alive while its run loop runs.
            unsafe { ffi::CGEventTapEnable(tap, true) };
        }
        ctx.state.lock().reset();
        return event;
    }
    if event.is_null() {
        return event;
    }
    // SAFETY: `event` is a valid event for the duration of the callback.
    let (marker, flags) = unsafe {
        (ffi::CGEventGetIntegerValueField(event, ffi::kCGEventSourceUserData), ffi::CGEventGetFlags(event))
    };
    if marker == EVENT_MARKER {
        return event; // our own ⌘V / ⌘C
    }
    // SAFETY: as above.
    let field = |f: u32| unsafe { ffi::CGEventGetIntegerValueField(event, f) };
    let keycode = || field(ffi::kCGKeyboardEventKeycode) as u16;
    let (key, down, repeat, transient) = match kind {
        ffi::kCGEventKeyDown => (Key::Code(keycode()), true, field(ffi::kCGKeyboardEventAutorepeat) != 0, false),
        ffi::kCGEventKeyUp => (Key::Code(keycode()), false, false, false),
        ffi::kCGEventFlagsChanged => {
            let code = keycode();
            let Some(down) = modifier_down(code, flags) else { return event };
            (Key::Code(code), down, false, false)
        }
        ffi::kCGEventOtherMouseDown => (Key::Mouse(field(ffi::kCGMouseEventButtonNumber)), true, false, false),
        ffi::kCGEventOtherMouseUp => (Key::Mouse(field(ffi::kCGMouseEventButtonNumber)), false, false, false),
        ffi::kCGEventLeftMouseDown => (Key::Mouse(0), true, false, true),
        ffi::kCGEventRightMouseDown => (Key::Mouse(1), true, false, true),
        _ => return event,
    };
    let swallow = {
        let mut state = ctx.state.lock();
        if kind != ffi::kCGEventFlagsChanged {
            state.resync(flags);
        }
        let swallow = state.handle(key, down, repeat, transient, super::PAUSED.load(Ordering::Relaxed));
        if state.any_active() {
            watch_held_keys(&ctx.state);
        }
        swallow
    };
    if swallow && ctx.can_swallow {
        ptr::null_mut()
    } else {
        event
    }
}

/// While a shortcut is held, checks the keyboard a few times a second: if macOS dropped the
/// key-up of a modifier (it happens, rarely), the shortcut is released anyway instead of
/// recording until the key is pressed again.
fn watch_held_keys(state: &Arc<Mutex<State>>) {
    static WATCHING: AtomicBool = AtomicBool::new(false);
    if WATCHING.swap(true, Ordering::SeqCst) {
        return;
    }
    let state = state.clone();
    let spawned = thread::Builder::new().name("od-hotkey-watch".into()).spawn(move || {
        loop {
            thread::sleep(Duration::from_millis(120));
            let mut st = state.lock();
            for key in st.stale_modifiers() {
                log::debug!("hotkeys: missed the release of {key:?}");
                st.handle(key, false, false, false, super::PAUSED.load(Ordering::Relaxed));
            }
            if !st.any_active() {
                WATCHING.store(false, Ordering::SeqCst);
                return;
            }
        }
    });
    if spawned.is_err() {
        WATCHING.store(false, Ordering::SeqCst);
    }
}

/// A running tap thread.
struct Running {
    run_loop: usize,
    can_swallow: bool,
}

/// Whether macOS currently lets us read keyboard events.
fn permitted() -> (bool, bool) {
    let trusted = crate::macos::permissions::accessibility_trusted();
    // SAFETY: plain query.
    let listen = trusted || unsafe { ffi::CGPreflightListenEventAccess() };
    (listen, trusted)
}

fn start_tap(state: Arc<Mutex<State>>, active: bool) -> Result<Running, String> {
    let (ready_tx, ready_rx) = mpsc::channel::<Result<usize, String>>();
    thread::Builder::new()
        .name("od-hotkey-tap".into())
        .spawn(move || {
            let ctx = Box::into_raw(Box::new(TapContext { state, tap: Mutex::new(ptr::null_mut()), can_swallow: active }));
            let mask: u64 = [
                ffi::kCGEventKeyDown,
                ffi::kCGEventKeyUp,
                ffi::kCGEventFlagsChanged,
                ffi::kCGEventLeftMouseDown,
                ffi::kCGEventRightMouseDown,
                ffi::kCGEventOtherMouseDown,
                ffi::kCGEventOtherMouseUp,
            ]
            .iter()
            .fold(0, |m, t| m | (1u64 << t));
            let options = if active { ffi::kCGEventTapOptionDefault } else { ffi::kCGEventTapOptionListenOnly };
            // SAFETY: standard event tap setup; `ctx` outlives the run loop and is freed below.
            unsafe {
                let tap = ffi::CGEventTapCreate(ffi::kCGSessionEventTap, ffi::kCGHeadInsertEventTap, options, mask, tap_callback, ctx as *mut c_void);
                if tap.is_null() {
                    let _ = ready_tx.send(Err("macOS did not allow reading the keyboard".into()));
                    drop(Box::from_raw(ctx));
                    return;
                }
                *(*ctx).tap.lock() = tap;
                let source = ffi::CFMachPortCreateRunLoopSource(ptr::null(), tap, 0);
                let run_loop = ffi::CFRunLoopGetCurrent();
                ffi::CFRunLoopAddSource(run_loop, source, ffi::kCFRunLoopCommonModes);
                ffi::CGEventTapEnable(tap, true);
                let _ = ready_tx.send(Ok(run_loop as usize));
                ffi::CFRunLoopRun();
                ffi::CGEventTapEnable(tap, false);
                ffi::CFMachPortInvalidate(tap);
                ffi::CFRelease(source as ffi::CFTypeRef);
                ffi::CFRelease(tap as ffi::CFTypeRef);
                drop(Box::from_raw(ctx));
            }
            log::info!("hotkeys: event tap stopped");
        })
        .map_err(|e| e.to_string())?;
    let run_loop = ready_rx.recv_timeout(Duration::from_secs(3)).map_err(|_| "the event tap did not start".to_string())??;
    Ok(Running { run_loop, can_swallow: active })
}

fn stop_tap(running: Running) {
    // SAFETY: CFRunLoopStop may be called from any thread; the run loop is alive until it returns.
    unsafe {
        ffi::CFRunLoopStop(running.run_loop as ffi::CFRunLoopRef);
        ffi::CFRunLoopWakeUp(running.run_loop as ffi::CFRunLoopRef);
    }
}

/// Outcome of [`TapListener::configure`].
pub enum TapState {
    /// No shortcut needs the tap.
    Idle,
    Running,
    /// Waiting for Accessibility / Input Monitoring permission.
    NeedsPermission,
    Failed(String),
}

pub struct TapListener {
    state: Arc<Mutex<State>>,
    running: Arc<Mutex<Option<Running>>>,
    /// Bumped on every configure, so an old permission watcher stops.
    generation: Arc<AtomicU64>,
}

impl TapListener {
    pub fn new() -> Self {
        Self { state: Arc::new(Mutex::new(State::default())), running: Arc::new(Mutex::new(None)), generation: Arc::new(AtomicU64::new(0)) }
    }

    /// Installs `bindings`, starting or stopping the tap as needed. Shortcuts that can't be
    /// expressed are returned as errors.
    pub fn configure(&self, bindings: &[Binding], tx: EventTx, status: Arc<Mutex<HotkeyStatus>>) -> (TapState, Vec<String>) {
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let mut errors = Vec::new();
        let compiled: Vec<TapBinding> = bindings
            .iter()
            .filter_map(|b| {
                let c = TapBinding::compile(b);
                if c.is_none() {
                    errors.push(format!("{}: unsupported key in '{}'", b.slot.id(), b.tokens.join("+")));
                }
                c
            })
            .collect();
        let wants_swallow = compiled.iter().any(|b| !b.modifier_only);
        {
            let mut st = self.state.lock();
            st.reset();
            st.bindings = compiled;
            st.tx = Some(tx);
        }
        if self.state.lock().bindings.is_empty() {
            if let Some(r) = self.running.lock().take() {
                stop_tap(r);
            }
            return (TapState::Idle, errors);
        }
        match self.ensure_running(wants_swallow) {
            Ok(()) => (TapState::Running, errors),
            Err(None) => {
                self.watch_permission(generation, wants_swallow, status);
                (TapState::NeedsPermission, errors)
            }
            Err(Some(e)) => (TapState::Failed(e), errors),
        }
    }

    /// Starts the tap if allowed. `Err(None)` means the permission is missing.
    fn ensure_running(&self, wants_swallow: bool) -> Result<(), Option<String>> {
        let (listen, trusted) = permitted();
        if !listen {
            if let Some(r) = self.running.lock().take() {
                stop_tap(r);
            }
            return Err(None);
        }
        let active = wants_swallow && trusted;
        let mut running = self.running.lock();
        if let Some(r) = running.as_ref() {
            if r.can_swallow == active {
                return Ok(());
            }
        }
        if let Some(r) = running.take() {
            stop_tap(r);
        }
        match start_tap(self.state.clone(), active) {
            Ok(r) => {
                log::info!("hotkeys: event tap running ({})", if active { "active" } else { "listen-only" });
                *running = Some(r);
                Ok(())
            }
            Err(e) => Err(Some(e)),
        }
    }

    fn watch_permission(&self, generation: u64, wants_swallow: bool, status: Arc<Mutex<HotkeyStatus>>) {
        let gen = self.generation.clone();
        let state = self.state.clone();
        let running = self.running.clone();
        let generation_now = generation;
        let _ = thread::Builder::new().name("od-hotkey-permission".into()).spawn(move || {
            let listener = TapListener { state, running, generation: gen.clone() };
            loop {
                thread::sleep(Duration::from_secs(1));
                if gen.load(Ordering::SeqCst) != generation_now {
                    return;
                }
                if permitted().0 {
                    match listener.ensure_running(wants_swallow) {
                        Ok(()) => {
                            log::info!("hotkeys: permission granted, single-key shortcuts are live");
                            let mut s = status.lock();
                            s.ok = true;
                            s.message = "Global shortcuts registered.".into();
                        }
                        Err(e) => {
                            let mut s = status.lock();
                            s.ok = false;
                            s.message = e.unwrap_or_else(|| "macOS did not allow reading the keyboard".into());
                        }
                    }
                    return;
                }
            }
        });
    }

    pub fn stop(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        if let Some(r) = self.running.lock().take() {
            stop_tap(r);
        }
        self.state.lock().reset();
        self.state.lock().bindings.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hotkeys::{parse, Slot};

    fn state(binds: &[(&str, Slot)]) -> (State, tokio::sync::mpsc::UnboundedReceiver<HotkeyEvent>) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let bindings = binds.iter().map(|(a, s)| TapBinding::compile(&Binding { slot: *s, tokens: parse(a) }).unwrap()).collect();
        (State { bindings, tx: Some(tx), ..Default::default() }, rx)
    }

    fn drain(rx: &mut tokio::sync::mpsc::UnboundedReceiver<HotkeyEvent>) -> Vec<HotkeyEvent> {
        let mut v = Vec::new();
        while let Ok(e) = rx.try_recv() {
            v.push(e);
        }
        v
    }

    const RCMD: Key = Key::Code(KVK_RIGHT_CMD);

    #[test]
    fn right_command_tap_and_hold() {
        let (mut st, mut rx) = state(&[("RightSuper", Slot::Shared)]);
        assert!(!st.handle(RCMD, true, false, false, false), "modifiers are never swallowed");
        assert!(!st.handle(RCMD, false, false, false, false));
        assert_eq!(drain(&mut rx), vec![HotkeyEvent::Pressed(Slot::Shared), HotkeyEvent::Released(Slot::Shared)]);
    }

    #[test]
    fn command_shortcut_interrupts() {
        let (mut st, mut rx) = state(&[("RightSuper", Slot::Shared)]);
        st.handle(RCMD, true, false, false, false);
        st.handle(Key::Code(8), true, false, false, false); // C
        st.handle(Key::Code(8), false, false, false, false);
        st.handle(RCMD, false, false, false, false);
        assert_eq!(drain(&mut rx), vec![HotkeyEvent::Pressed(Slot::Shared), HotkeyEvent::Interrupted(Slot::Shared)]);
        // Works again afterwards.
        st.handle(RCMD, true, false, false, false);
        assert_eq!(drain(&mut rx), vec![HotkeyEvent::Pressed(Slot::Shared)]);
    }

    #[test]
    fn command_click_interrupts() {
        let (mut st, mut rx) = state(&[("RightSuper", Slot::Shared)]);
        st.handle(RCMD, true, false, false, false);
        st.handle(Key::Mouse(0), true, false, true, false);
        assert_eq!(drain(&mut rx), vec![HotkeyEvent::Pressed(Slot::Shared), HotkeyEvent::Interrupted(Slot::Shared)]);
    }

    #[test]
    fn left_command_does_not_trigger_right() {
        let (mut st, mut rx) = state(&[("RightSuper", Slot::Shared)]);
        st.handle(Key::Code(KVK_CMD), true, false, false, false);
        st.handle(Key::Code(KVK_CMD), false, false, false, false);
        assert!(drain(&mut rx).is_empty());
    }

    #[test]
    fn extra_modifier_does_not_match() {
        let (mut st, mut rx) = state(&[("RightSuper", Slot::Shared)]);
        st.handle(Key::Code(KVK_SHIFT), true, false, false, false);
        st.handle(RCMD, true, false, false, false);
        assert!(drain(&mut rx).is_empty());
    }

    #[test]
    fn fn_key_and_mouse_buttons() {
        let (mut st, mut rx) = state(&[("Fn", Slot::Dictate), ("Mouse4", Slot::Command)]);
        st.handle(Key::Code(KVK_FUNCTION), true, false, false, false);
        st.handle(Key::Code(KVK_FUNCTION), false, false, false, false);
        assert!(st.handle(Key::Mouse(3), true, false, false, false), "bound mouse buttons are swallowed");
        assert!(st.handle(Key::Mouse(3), false, false, false, false));
        assert_eq!(
            drain(&mut rx),
            vec![
                HotkeyEvent::Pressed(Slot::Dictate),
                HotkeyEvent::Released(Slot::Dictate),
                HotkeyEvent::Pressed(Slot::Command),
                HotkeyEvent::Released(Slot::Command)
            ]
        );
    }

    #[test]
    fn combo_key_is_swallowed_with_its_repeats() {
        let (mut st, mut rx) = state(&[("Alt+Space", Slot::Dictate)]);
        st.handle(Key::Code(KVK_OPTION), true, false, false, false);
        assert!(st.handle(Key::Code(49), true, false, false, false));
        assert!(st.handle(Key::Code(49), true, true, false, false), "auto-repeat is swallowed too");
        assert!(st.handle(Key::Code(49), false, false, false, false));
        assert!(!st.handle(Key::Code(KVK_OPTION), false, false, false, false));
        assert_eq!(drain(&mut rx), vec![HotkeyEvent::Pressed(Slot::Dictate), HotkeyEvent::Released(Slot::Dictate)]);
    }

    #[test]
    fn paused_passes_everything_through() {
        let (mut st, mut rx) = state(&[("Alt+Space", Slot::Dictate)]);
        st.handle(Key::Code(KVK_OPTION), true, false, false, true);
        assert!(!st.handle(Key::Code(49), true, false, false, true));
        assert!(drain(&mut rx).is_empty());
    }

    #[test]
    fn flags_tell_left_and_right_apart() {
        use ffi::*;
        assert_eq!(modifier_down(KVK_RIGHT_CMD, kCGEventFlagMaskCommand | NX_DEVICERCMDKEYMASK), Some(true));
        // Right ⌘ released while left ⌘ is still down.
        assert_eq!(modifier_down(KVK_RIGHT_CMD, kCGEventFlagMaskCommand | NX_DEVICELCMDKEYMASK), Some(false));
        // Keyboards without side bits.
        assert_eq!(modifier_down(KVK_RIGHT_OPTION, kCGEventFlagMaskAlternate), Some(true));
        assert_eq!(modifier_down(KVK_RIGHT_OPTION, 0), Some(false));
        assert_eq!(modifier_down(KVK_FUNCTION, kCGEventFlagMaskSecondaryFn), Some(true));
        assert_eq!(modifier_down(57, 0), None); // caps lock
    }

    #[test]
    fn tokens_map_to_key_codes() {
        assert_eq!(token_keys("V"), Some(vec![Key::Code(9)]));
        assert_eq!(token_keys("1"), Some(vec![Key::Code(18)]));
        assert_eq!(token_keys("F13"), Some(vec![Key::Code(105)]));
        assert_eq!(token_keys("Numpad5"), Some(vec![Key::Code(87)]));
        assert!(needs_tap(&Binding { slot: Slot::Shared, tokens: parse("RightSuper") }));
        assert!(needs_tap(&Binding { slot: Slot::Shared, tokens: parse("Fn") }));
        assert!(!needs_tap(&Binding { slot: Slot::Shared, tokens: parse("Alt+Space") }));
    }
}
