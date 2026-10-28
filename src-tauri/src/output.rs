//! Clipboard access, selection capture and text insertion into the focused application.

use std::{thread, time::Duration};

use anyhow::{anyhow, Result};
use parking_lot::Mutex;

use crate::settings::{OutputMethod, OutputSettings, PasteShortcut, SelectionMethod};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chord {
    Paste(PasteShortcut),
    Copy,
    /// Terminals copy with Ctrl+Shift+C (Ctrl+C would interrupt the running program).
    CopyTerminal,
}

// ---------------------------------------------------------------------------------------------
// Key injection backends
// ---------------------------------------------------------------------------------------------

#[cfg(target_os = "linux")]
mod keys {
    use std::{thread, time::Duration};

    use anyhow::{Context, Result};
    use evdev::{uinput::VirtualDevice, AttributeSet, EventType, InputEvent, KeyCode};

    use super::Chord;
    use crate::settings::PasteShortcut;

    pub const DEVICE_NAME: &str = "OpenDictate Virtual Keyboard";

    pub struct Keys {
        dev: VirtualDevice,
    }

    impl Keys {
        pub fn new() -> Result<Self> {
            let mut keys = AttributeSet::<KeyCode>::new();
            // Advertise a full keyboard so compositors treat the device as one.
            for code in 1..=248u16 {
                keys.insert(KeyCode::new(code));
            }
            let dev = VirtualDevice::builder()
                .context("cannot open /dev/uinput")?
                .name(DEVICE_NAME)
                .with_keys(&keys)?
                .build()
                .context("cannot create the virtual keyboard")?;
            Ok(Self { dev })
        }

        fn key(&mut self, code: KeyCode, down: bool) -> Result<()> {
            self.dev.emit(&[InputEvent::new(EventType::KEY.0, code.0, down as i32)])?;
            thread::sleep(Duration::from_millis(6));
            Ok(())
        }

        pub fn chord(&mut self, chord: Chord) -> Result<()> {
            let (mods, key): (&[KeyCode], KeyCode) = match chord {
                Chord::Paste(PasteShortcut::CtrlV | PasteShortcut::Auto) => (&[KeyCode::KEY_LEFTCTRL], KeyCode::KEY_V),
                Chord::Paste(PasteShortcut::CtrlShiftV) => (&[KeyCode::KEY_LEFTCTRL, KeyCode::KEY_LEFTSHIFT], KeyCode::KEY_V),
                Chord::Paste(PasteShortcut::ShiftInsert) => (&[KeyCode::KEY_LEFTSHIFT], KeyCode::KEY_INSERT),
                Chord::Copy => (&[KeyCode::KEY_LEFTCTRL], KeyCode::KEY_C),
                Chord::CopyTerminal => (&[KeyCode::KEY_LEFTCTRL, KeyCode::KEY_LEFTSHIFT], KeyCode::KEY_C),
            };
            for m in mods {
                self.key(*m, true)?;
            }
            self.key(key, true)?;
            self.key(key, false)?;
            for m in mods.iter().rev() {
                self.key(*m, false)?;
            }
            Ok(())
        }

        pub fn type_text(&mut self, _text: &str) -> Result<bool> {
            // Typing arbitrary Unicode through uinput depends on the active keyboard layout;
            // the caller falls back to pasting.
            Ok(false)
        }
    }
}

#[cfg(not(target_os = "linux"))]
mod keys {
    use anyhow::{anyhow, Result};
    use enigo::{Direction, Enigo, Key, Keyboard, Settings};

    use super::Chord;
    use crate::settings::PasteShortcut;

    pub struct Keys {
        enigo: Enigo,
    }

    // Enigo holds platform handles that are only used from behind our mutex.
    unsafe impl Send for Keys {}

    impl Keys {
        pub fn new() -> Result<Self> {
            let enigo = Enigo::new(&Settings::default()).map_err(|e| anyhow!("cannot simulate input: {e:?}"))?;
            Ok(Self { enigo })
        }

        pub fn chord(&mut self, chord: Chord) -> Result<()> {
            #[cfg(target_os = "macos")]
            let primary = Key::Meta;
            #[cfg(not(target_os = "macos"))]
            let primary = Key::Control;
            let (mods, key): (Vec<Key>, Key) = match chord {
                Chord::Paste(PasteShortcut::CtrlV | PasteShortcut::Auto) => (vec![primary], Key::Unicode('v')),
                Chord::Paste(PasteShortcut::CtrlShiftV) => (vec![primary, Key::Shift], Key::Unicode('v')),
                #[cfg(not(target_os = "macos"))]
                Chord::Paste(PasteShortcut::ShiftInsert) => (vec![Key::Shift], Key::Insert),
                #[cfg(target_os = "macos")]
                Chord::Paste(PasteShortcut::ShiftInsert) => (vec![primary], Key::Unicode('v')),
                Chord::Copy => (vec![primary], Key::Unicode('c')),
                Chord::CopyTerminal => (vec![primary, Key::Shift], Key::Unicode('c')),
            };
            let e = |r: Result<(), enigo::InputError>| r.map_err(|e| anyhow!("input simulation failed: {e:?}"));
            for m in &mods {
                e(self.enigo.key(*m, Direction::Press))?;
            }
            e(self.enigo.key(key, Direction::Click))?;
            for m in mods.iter().rev() {
                e(self.enigo.key(*m, Direction::Release))?;
            }
            Ok(())
        }

        pub fn type_text(&mut self, text: &str) -> Result<bool> {
            self.enigo.text(text).map_err(|e| anyhow!("typing failed: {e:?}"))?;
            Ok(true)
        }
    }
}

#[cfg(target_os = "linux")]
pub use keys::DEVICE_NAME as VIRTUAL_KEYBOARD_NAME;

// ---------------------------------------------------------------------------------------------
// Output service
// ---------------------------------------------------------------------------------------------

pub struct Output {
    keys: Mutex<Option<keys::Keys>>,
    keys_error: Mutex<Option<String>>,
    clipboard: Mutex<Option<arboard::Clipboard>>,
}

impl Output {
    pub fn new() -> Self {
        let out = Self { keys: Mutex::new(None), keys_error: Mutex::new(None), clipboard: Mutex::new(None) };
        out.ensure_keys();
        out
    }

    /// Lazily creates the key injector (retrying after permission fixes).
    fn ensure_keys(&self) -> bool {
        let mut k = self.keys.lock();
        if k.is_some() {
            return true;
        }
        match keys::Keys::new() {
            Ok(dev) => {
                *k = Some(dev);
                *self.keys_error.lock() = None;
                // Give the compositor a moment to register a freshly created device.
                #[cfg(target_os = "linux")]
                thread::sleep(Duration::from_millis(150));
                true
            }
            Err(e) => {
                log::warn!("key injection unavailable: {e:#}");
                *self.keys_error.lock() = Some(format!("{e:#}"));
                false
            }
        }
    }

    pub fn can_inject(&self) -> bool {
        self.ensure_keys()
    }

    pub fn injection_error(&self) -> Option<String> {
        self.keys_error.lock().clone()
    }

    fn chord(&self, chord: Chord) -> Result<()> {
        if !self.ensure_keys() {
            return Err(anyhow!(
                "Cannot simulate key presses: {}",
                self.keys_error.lock().clone().unwrap_or_default()
            ));
        }
        self.keys.lock().as_mut().unwrap().chord(chord)
    }

    fn with_clipboard<T>(&self, f: impl FnOnce(&mut arboard::Clipboard) -> Result<T, arboard::Error>) -> Result<T> {
        let mut guard = self.clipboard.lock();
        if guard.is_none() {
            *guard = Some(arboard::Clipboard::new().map_err(|e| anyhow!("clipboard unavailable: {e}"))?);
        }
        f(guard.as_mut().unwrap()).map_err(|e| anyhow!("clipboard error: {e}"))
    }

    pub fn get_clipboard(&self) -> Option<String> {
        self.with_clipboard(|c| c.get_text()).ok()
    }

    /// Sets the clipboard (e.g. when the user clicks "Copy").
    pub fn set_clipboard(&self, text: &str) -> Result<()> {
        self.with_clipboard(|c| c.set_text(text.to_string()))
    }

    /// Sets transient clipboard content (probes, text being pasted) and asks clipboard
    /// managers such as Klipper not to record it in their history.
    fn set_clipboard_private(&self, text: &str) -> Result<()> {
        let text = text.to_string();
        self.with_clipboard(move |c| {
            #[cfg(target_os = "linux")]
            {
                use arboard::SetExtLinux;
                c.set().exclude_from_history().text(text)
            }
            #[cfg(target_os = "macos")]
            {
                use arboard::SetExtApple;
                c.set().exclude_from_history().text(text)
            }
            #[cfg(target_os = "windows")]
            {
                use arboard::SetExtWindows;
                c.set().exclude_from_history().exclude_from_cloud().text(text)
            }
        })
    }

    fn clear_clipboard(&self) {
        let _ = self.with_clipboard(|c| c.clear());
    }

    #[cfg(target_os = "linux")]
    fn get_primary(&self) -> Option<String> {
        use arboard::{GetExtLinux, LinuxClipboardKind};
        self.with_clipboard(|c| c.get().clipboard(LinuxClipboardKind::Primary).text()).ok()
    }

    #[cfg(not(target_os = "linux"))]
    fn get_primary(&self) -> Option<String> {
        None
    }

    /// Reads the text currently selected in the focused application.
    /// Returns the selection and the previous clipboard text (to restore later).
    pub fn capture_selection(&self, method: SelectionMethod, terminal: bool) -> (Option<String>, Option<String>) {
        match method {
            SelectionMethod::None => (None, None),
            SelectionMethod::Primary => (self.get_primary().filter(|s| !s.trim().is_empty()), None),
            SelectionMethod::Copy => {
                let previous = self.get_clipboard();
                // Never clear the clipboard here: clipboard managers (e.g. Klipper) refill an
                // empty clipboard from their history, which would look like a selection.
                let sentinel = format!("\u{2063}opendictate-probe-{}", crate::history::now_ms());
                if let Err(e) = self.set_clipboard_private(&sentinel) {
                    log::warn!("clipboard unavailable: {e:#}");
                    return (None, previous);
                }
                thread::sleep(Duration::from_millis(40));
                if let Err(e) = self.chord(if terminal { Chord::CopyTerminal } else { Chord::Copy }) {
                    log::warn!("could not send the copy shortcut: {e:#}");
                    self.restore(previous.clone());
                    return (None, previous);
                }
                let mut selection = None;
                for _ in 0..20 {
                    thread::sleep(Duration::from_millis(20));
                    match self.get_clipboard() {
                        Some(t) if t != sentinel && !t.is_empty() => {
                            selection = Some(t);
                            break;
                        }
                        _ => {}
                    }
                }
                self.restore(previous.clone());
                (selection.filter(|s| !s.trim().is_empty()), previous)
            }
        }
    }

    fn restore(&self, previous: Option<String>) {
        match previous {
            Some(p) => {
                let _ = self.set_clipboard_private(&p);
            }
            None => self.clear_clipboard(),
        }
    }

    /// Inserts text at the cursor of the focused application.
    pub fn insert(&self, text: &str, cfg: &OutputSettings, terminal: bool) -> Result<InsertOutcome> {
        if text.is_empty() {
            return Ok(InsertOutcome::Inserted);
        }
        match cfg.method {
            OutputMethod::Clipboard => {
                // The user wants this one on the clipboard (and in its history).
                self.set_clipboard(text)?;
                Ok(InsertOutcome::CopiedOnly)
            }
            OutputMethod::Type => {
                if self.ensure_keys() {
                    if let Some(k) = self.keys.lock().as_mut() {
                        if k.type_text(text)? {
                            return Ok(InsertOutcome::Inserted);
                        }
                    }
                }
                self.paste(text, cfg, terminal)
            }
            OutputMethod::Paste => self.paste(text, cfg, terminal),
        }
    }

    fn paste(&self, text: &str, cfg: &OutputSettings, terminal: bool) -> Result<InsertOutcome> {
        if !self.ensure_keys() {
            // Leave the text on the clipboard so nothing is lost.
            self.set_clipboard(text)?;
            return Ok(InsertOutcome::CopiedOnly);
        }
        let previous = if cfg.restore_clipboard { self.get_clipboard() } else { None };
        if cfg.restore_clipboard {
            self.set_clipboard_private(text)?;
        } else {
            self.set_clipboard(text)?;
        }
        // Let the compositor see the new clipboard owner before pasting.
        thread::sleep(Duration::from_millis(40));
        let shortcut = match cfg.paste_shortcut {
            PasteShortcut::Auto if terminal => PasteShortcut::CtrlShiftV,
            other => other,
        };
        self.chord(Chord::Paste(shortcut))?;
        if cfg.restore_clipboard {
            // Apps read the clipboard asynchronously after the key press.
            thread::sleep(Duration::from_millis(350));
            if let Some(p) = previous {
                let _ = self.set_clipboard_private(&p);
            }
        }
        Ok(InsertOutcome::Inserted)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertOutcome {
    Inserted,
    CopiedOnly,
}
