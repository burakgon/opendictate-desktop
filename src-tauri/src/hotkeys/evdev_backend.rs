//! Linux hotkeys read straight from `/dev/input/event*`.
//!
//! Keys are observed, never grabbed, so the focused application still receives them. This is
//! what allows modifier-only hotkeys (e.g. Right Ctrl) and reliable press/release detection on
//! every compositor, X11 included. It needs read access to input devices (`input` group).

use std::{
    collections::HashSet,
    fs,
    path::PathBuf,
    sync::{
        mpsc::{self, Sender},
        Arc,
    },
    thread,
    time::Duration,
};

use evdev::{Device, EventSummary, KeyCode};
use parking_lot::{Mutex, RwLock};

use super::{Binding, EventTx, HotkeyEvent};

/// Shared state of the (single, long-lived) listener.
pub struct EvdevListener {
    bindings: Arc<RwLock<Vec<CompiledBinding>>>,
    tx: Arc<RwLock<Option<EventTx>>>,
    enabled: Arc<RwLock<bool>>,
    pressed: Arc<Mutex<HashSet<KeyCode>>>,
    devices: Arc<Mutex<HashSet<PathBuf>>>,
}

#[derive(Debug, Clone)]
struct CompiledBinding {
    binding: Binding,
    /// Each token is satisfied by any one of these codes.
    tokens: Vec<Vec<KeyCode>>,
    /// Modifier classes this binding uses (so extra modifiers make it not match).
    mod_classes: HashSet<u8>,
    active: bool,
    suppressed: bool,
}

pub fn token_codes(token: &str) -> Option<Vec<KeyCode>> {
    use KeyCode as K;
    let one = |k: KeyCode| Some(vec![k]);
    match token {
        "Ctrl" => Some(vec![K::KEY_LEFTCTRL, K::KEY_RIGHTCTRL]),
        "Shift" => Some(vec![K::KEY_LEFTSHIFT, K::KEY_RIGHTSHIFT]),
        "Alt" => Some(vec![K::KEY_LEFTALT, K::KEY_RIGHTALT]),
        "Super" => Some(vec![K::KEY_LEFTMETA, K::KEY_RIGHTMETA]),
        "LeftCtrl" => one(K::KEY_LEFTCTRL),
        "RightCtrl" => one(K::KEY_RIGHTCTRL),
        "LeftShift" => one(K::KEY_LEFTSHIFT),
        "RightShift" => one(K::KEY_RIGHTSHIFT),
        "LeftAlt" => one(K::KEY_LEFTALT),
        "RightAlt" => one(K::KEY_RIGHTALT),
        "LeftSuper" => one(K::KEY_LEFTMETA),
        "RightSuper" => one(K::KEY_RIGHTMETA),
        "Space" => one(K::KEY_SPACE),
        "Enter" => one(K::KEY_ENTER),
        "Tab" => one(K::KEY_TAB),
        "Escape" => one(K::KEY_ESC),
        "Backspace" => one(K::KEY_BACKSPACE),
        "Insert" => one(K::KEY_INSERT),
        "Delete" => one(K::KEY_DELETE),
        "Home" => one(K::KEY_HOME),
        "End" => one(K::KEY_END),
        "PageUp" => one(K::KEY_PAGEUP),
        "PageDown" => one(K::KEY_PAGEDOWN),
        "Up" => one(K::KEY_UP),
        "Down" => one(K::KEY_DOWN),
        "Left" => one(K::KEY_LEFT),
        "Right" => one(K::KEY_RIGHT),
        "CapsLock" => one(K::KEY_CAPSLOCK),
        "ScrollLock" => one(K::KEY_SCROLLLOCK),
        "Pause" => one(K::KEY_PAUSE),
        "PrintScreen" => one(K::KEY_SYSRQ),
        "Menu" => one(K::KEY_COMPOSE),
        "`" => one(K::KEY_GRAVE),
        "-" => one(K::KEY_MINUS),
        "=" => one(K::KEY_EQUAL),
        "[" => one(K::KEY_LEFTBRACE),
        "]" => one(K::KEY_RIGHTBRACE),
        "\\" => one(K::KEY_BACKSLASH),
        ";" => one(K::KEY_SEMICOLON),
        "'" => one(K::KEY_APOSTROPHE),
        "," => one(K::KEY_COMMA),
        "." => one(K::KEY_DOT),
        "/" => one(K::KEY_SLASH),
        "MouseMiddle" => one(K::BTN_MIDDLE),
        "Mouse4" => one(K::BTN_SIDE),
        "Mouse5" => one(K::BTN_EXTRA),
        "MicMute" => one(K::KEY_MICMUTE),
        "Copilot" => one(K::KEY_F23),
        t if t.len() == 1 => {
            let c = t.chars().next()?.to_ascii_uppercase();
            let code = match c {
                'A' => K::KEY_A, 'B' => K::KEY_B, 'C' => K::KEY_C, 'D' => K::KEY_D, 'E' => K::KEY_E,
                'F' => K::KEY_F, 'G' => K::KEY_G, 'H' => K::KEY_H, 'I' => K::KEY_I, 'J' => K::KEY_J,
                'K' => K::KEY_K, 'L' => K::KEY_L, 'M' => K::KEY_M, 'N' => K::KEY_N, 'O' => K::KEY_O,
                'P' => K::KEY_P, 'Q' => K::KEY_Q, 'R' => K::KEY_R, 'S' => K::KEY_S, 'T' => K::KEY_T,
                'U' => K::KEY_U, 'V' => K::KEY_V, 'W' => K::KEY_W, 'X' => K::KEY_X, 'Y' => K::KEY_Y,
                'Z' => K::KEY_Z, '0' => K::KEY_0, '1' => K::KEY_1, '2' => K::KEY_2, '3' => K::KEY_3,
                '4' => K::KEY_4, '5' => K::KEY_5, '6' => K::KEY_6, '7' => K::KEY_7, '8' => K::KEY_8,
                '9' => K::KEY_9,
                _ => return None,
            };
            one(code)
        }
        t if t.starts_with('F') && t.len() <= 3 => {
            let n: u16 = t[1..].parse().ok()?;
            let code = match n {
                1..=10 => K::KEY_F1.0 + (n - 1),
                11 => K::KEY_F11.0,
                12 => K::KEY_F12.0,
                13..=24 => K::KEY_F13.0 + (n - 13),
                _ => return None,
            };
            one(KeyCode::new(code))
        }
        t if t.starts_with("Numpad") => {
            let rest = &t[6..];
            let code = match rest {
                "0" => K::KEY_KP0, "1" => K::KEY_KP1, "2" => K::KEY_KP2, "3" => K::KEY_KP3, "4" => K::KEY_KP4,
                "5" => K::KEY_KP5, "6" => K::KEY_KP6, "7" => K::KEY_KP7, "8" => K::KEY_KP8, "9" => K::KEY_KP9,
                "Enter" => K::KEY_KPENTER, "+" | "Add" => K::KEY_KPPLUS, "-" | "Subtract" => K::KEY_KPMINUS,
                "*" | "Multiply" => K::KEY_KPASTERISK, "/" | "Divide" => K::KEY_KPSLASH, "." | "Decimal" => K::KEY_KPDOT,
                _ => return None,
            };
            one(code)
        }
        _ => None,
    }
}

/// 0 = ctrl, 1 = shift, 2 = alt, 3 = super.
fn mod_class(code: KeyCode) -> Option<u8> {
    use KeyCode as K;
    match code {
        K::KEY_LEFTCTRL | K::KEY_RIGHTCTRL => Some(0),
        K::KEY_LEFTSHIFT | K::KEY_RIGHTSHIFT => Some(1),
        K::KEY_LEFTALT | K::KEY_RIGHTALT => Some(2),
        K::KEY_LEFTMETA | K::KEY_RIGHTMETA => Some(3),
        _ => None,
    }
}

fn compile(binding: &Binding) -> Option<CompiledBinding> {
    let mut tokens = Vec::new();
    let mut mod_classes = HashSet::new();
    for t in &binding.tokens {
        let codes = token_codes(t)?;
        if let Some(c) = mod_class(codes[0]) {
            mod_classes.insert(c);
        }
        tokens.push(codes);
    }
    Some(CompiledBinding { binding: binding.clone(), tokens, mod_classes, active: false, suppressed: false })
}

impl CompiledBinding {
    fn involves(&self, code: KeyCode) -> bool {
        self.tokens.iter().any(|t| t.contains(&code))
    }

    fn satisfied(&self, pressed: &HashSet<KeyCode>) -> bool {
        if !self.tokens.iter().all(|alts| alts.iter().any(|c| pressed.contains(c))) {
            return false;
        }
        // Extra modifiers (e.g. Ctrl+Shift+X when the binding is Ctrl+X) don't match.
        pressed.iter().filter_map(|c| mod_class(*c)).all(|class| self.mod_classes.contains(&class))
    }
}

impl EvdevListener {
    /// Starts watching input devices. Returns an error if no device is readable.
    pub fn start() -> anyhow::Result<Arc<Self>> {
        if !can_read_input_devices() {
            anyhow::bail!("no readable keyboard in /dev/input (add yourself to the 'input' group)");
        }
        let listener = Arc::new(Self {
            bindings: Arc::new(RwLock::new(Vec::new())),
            tx: Arc::new(RwLock::new(None)),
            enabled: Arc::new(RwLock::new(false)),
            pressed: Arc::new(Mutex::new(HashSet::new())),
            devices: Arc::new(Mutex::new(HashSet::new())),
        });
        let (raw_tx, raw_rx) = mpsc::channel::<(KeyCode, i32)>();

        // Matcher thread: owns the binding state machine.
        {
            let l = listener.clone();
            thread::Builder::new().name("od-hotkeys".into()).spawn(move || {
                for (code, value) in raw_rx {
                    l.handle(code, value);
                }
            })?;
        }
        // Device scanner: picks up hot-plugged keyboards.
        {
            let l = listener.clone();
            thread::Builder::new().name("od-hotkey-scan".into()).spawn(move || loop {
                l.scan(&raw_tx);
                thread::sleep(Duration::from_secs(3));
            })?;
        }
        Ok(listener)
    }

    pub fn configure(&self, bindings: &[Binding], tx: EventTx) -> Vec<String> {
        let mut errors = Vec::new();
        let mut compiled = Vec::new();
        for b in bindings {
            match compile(b) {
                Some(c) => compiled.push(c),
                None => errors.push(format!("unsupported key in '{}'", b.tokens.join("+"))),
            }
        }
        *self.bindings.write() = compiled;
        *self.tx.write() = Some(tx);
        *self.enabled.write() = true;
        errors
    }

    pub fn disable(&self) {
        *self.enabled.write() = false;
        for b in self.bindings.write().iter_mut() {
            b.active = false;
        }
    }

    fn scan(&self, raw_tx: &Sender<(KeyCode, i32)>) {
        let Ok(entries) = fs::read_dir("/dev/input") else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            let is_event = path.file_name().and_then(|n| n.to_str()).map(|n| n.starts_with("event")).unwrap_or(false);
            if !is_event || self.devices.lock().contains(&path) {
                continue;
            }
            let Ok(device) = Device::open(&path) else { continue };
            if device.name() == Some(crate::output::VIRTUAL_KEYBOARD_NAME) {
                continue;
            }
            let interesting = device
                .supported_keys()
                .map(|k| {
                    k.contains(KeyCode::KEY_A)
                        || k.contains(KeyCode::KEY_SPACE)
                        || k.contains(KeyCode::BTN_SIDE)
                        || k.contains(KeyCode::KEY_MICMUTE)
                })
                .unwrap_or(false);
            if !interesting {
                continue;
            }
            log::info!("hotkeys: watching {} ({})", path.display(), device.name().unwrap_or("?"));
            self.devices.lock().insert(path.clone());
            let tx = raw_tx.clone();
            let devices = self.devices.clone();
            let _ = thread::Builder::new().name("od-evdev".into()).spawn(move || {
                read_device(device, tx);
                devices.lock().remove(&path);
            });
        }
    }

    fn handle(&self, code: KeyCode, value: i32) {
        if value == 2 {
            return; // auto-repeat
        }
        let down = value == 1;
        let pressed_snapshot = {
            let mut pressed = self.pressed.lock();
            if down {
                pressed.insert(code);
            } else {
                pressed.remove(&code);
            }
            pressed.clone()
        };
        if !*self.enabled.read() {
            return;
        }
        let Some(tx) = self.tx.read().clone() else { return };
        let mut bindings = self.bindings.write();
        let mut consumed_escape = false;

        for b in bindings.iter_mut() {
            let sat = b.satisfied(&pressed_snapshot);
            if b.suppressed {
                if !b.tokens.iter().any(|alts| alts.iter().any(|c| pressed_snapshot.contains(c))) {
                    b.suppressed = false;
                }
                continue;
            }
            if b.active {
                if down && !b.involves(code) && b.binding.modifier_only() {
                    // e.g. Right Ctrl held and then C pressed: this was a normal shortcut.
                    b.active = false;
                    b.suppressed = true;
                    let _ = tx.send(HotkeyEvent::Interrupted(b.binding.slot));
                    if code == KeyCode::KEY_ESC {
                        consumed_escape = true;
                    }
                } else if !sat {
                    b.active = false;
                    let _ = tx.send(HotkeyEvent::Released(b.binding.slot));
                }
            } else if sat && down && b.involves(code) {
                b.active = true;
                let _ = tx.send(HotkeyEvent::Pressed(b.binding.slot));
            }
        }
        if down && code == KeyCode::KEY_ESC && !consumed_escape {
            let _ = tx.send(HotkeyEvent::Escape);
        }
    }
}

fn read_device(mut device: Device, tx: Sender<(KeyCode, i32)>) {
    loop {
        match device.fetch_events() {
            Ok(events) => {
                for ev in events {
                    if let EventSummary::Key(_, code, value) = ev.destructure() {
                        if tx.send((code, value)).is_err() {
                            return;
                        }
                    }
                }
            }
            Err(e) => {
                log::info!("hotkeys: device closed ({e})");
                return;
            }
        }
    }
}

/// True if at least one keyboard under /dev/input can be opened.
pub fn can_read_input_devices() -> bool {
    let Ok(entries) = fs::read_dir("/dev/input") else { return false };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.file_name().and_then(|n| n.to_str()).map(|n| n.starts_with("event")).unwrap_or(false) {
            continue;
        }
        if let Ok(d) = Device::open(&path) {
            if d.supported_keys().map(|k| k.contains(KeyCode::KEY_A)).unwrap_or(false) {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hotkeys::{parse, Slot};

    fn listener() -> (EvdevListener, tokio::sync::mpsc::UnboundedReceiver<HotkeyEvent>) {
        let l = EvdevListener {
            bindings: Arc::new(RwLock::new(Vec::new())),
            tx: Arc::new(RwLock::new(None)),
            enabled: Arc::new(RwLock::new(false)),
            pressed: Arc::new(Mutex::new(HashSet::new())),
            devices: Arc::new(Mutex::new(HashSet::new())),
        };
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        l.configure(
            &[
                Binding { slot: Slot::Dictate, tokens: parse("RightCtrl") },
                Binding { slot: Slot::Command, tokens: parse("Ctrl+Alt+Space") },
            ],
            tx,
        );
        (l, rx)
    }

    fn drain(rx: &mut tokio::sync::mpsc::UnboundedReceiver<HotkeyEvent>) -> Vec<HotkeyEvent> {
        let mut v = Vec::new();
        while let Ok(e) = rx.try_recv() {
            v.push(e);
        }
        v
    }

    #[test]
    fn modifier_only_press_release() {
        let (l, mut rx) = listener();
        l.handle(KeyCode::KEY_RIGHTCTRL, 1);
        l.handle(KeyCode::KEY_RIGHTCTRL, 2);
        l.handle(KeyCode::KEY_RIGHTCTRL, 0);
        assert_eq!(drain(&mut rx), vec![HotkeyEvent::Pressed(Slot::Dictate), HotkeyEvent::Released(Slot::Dictate)]);
    }

    #[test]
    fn modifier_only_interrupted_by_shortcut() {
        let (l, mut rx) = listener();
        l.handle(KeyCode::KEY_RIGHTCTRL, 1);
        l.handle(KeyCode::KEY_C, 1);
        l.handle(KeyCode::KEY_C, 0);
        l.handle(KeyCode::KEY_RIGHTCTRL, 0);
        assert_eq!(drain(&mut rx), vec![HotkeyEvent::Pressed(Slot::Dictate), HotkeyEvent::Interrupted(Slot::Dictate)]);
        // Works again afterwards.
        l.handle(KeyCode::KEY_RIGHTCTRL, 1);
        assert_eq!(drain(&mut rx), vec![HotkeyEvent::Pressed(Slot::Dictate)]);
    }

    #[test]
    fn combo_with_generic_modifiers() {
        let (l, mut rx) = listener();
        l.handle(KeyCode::KEY_LEFTCTRL, 1);
        l.handle(KeyCode::KEY_LEFTALT, 1);
        l.handle(KeyCode::KEY_SPACE, 1);
        l.handle(KeyCode::KEY_SPACE, 0);
        l.handle(KeyCode::KEY_LEFTALT, 0);
        l.handle(KeyCode::KEY_LEFTCTRL, 0);
        assert_eq!(drain(&mut rx), vec![HotkeyEvent::Pressed(Slot::Command), HotkeyEvent::Released(Slot::Command)]);
    }

    #[test]
    fn extra_modifier_does_not_match() {
        let (l, mut rx) = listener();
        l.handle(KeyCode::KEY_LEFTCTRL, 1);
        l.handle(KeyCode::KEY_LEFTSHIFT, 1);
        l.handle(KeyCode::KEY_LEFTALT, 1);
        l.handle(KeyCode::KEY_SPACE, 1);
        assert!(drain(&mut rx).is_empty());
    }

    #[test]
    fn escape_is_reported() {
        let (l, mut rx) = listener();
        l.handle(KeyCode::KEY_ESC, 1);
        assert_eq!(drain(&mut rx), vec![HotkeyEvent::Escape]);
    }
}
