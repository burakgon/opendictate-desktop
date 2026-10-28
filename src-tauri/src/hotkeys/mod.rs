//! Global hotkeys. Backends turn key presses into [`HotkeyEvent`]s for the engine.
//!
//! * Linux: `evdev` (reads keyboards directly; supports modifier-only keys and hold-to-talk)
//!   or the XDG `GlobalShortcuts` portal (no permissions needed).
//! * macOS / Windows: native global shortcuts.

use serde::Serialize;
use tokio::sync::mpsc::UnboundedSender;

#[cfg(target_os = "linux")]
pub mod evdev_backend;
pub mod manager;
#[cfg(not(target_os = "linux"))]
pub mod native_backend;
#[cfg(target_os = "linux")]
pub mod portal_backend;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Slot {
    Dictate,
    Command,
    /// Dictate and command share a key: tap dictates, hold runs a command.
    Shared,
}

impl Slot {
    pub fn id(self) -> &'static str {
        match self {
            Slot::Dictate => "dictate",
            Slot::Command => "command",
            Slot::Shared => "shared",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyEvent {
    Pressed(Slot),
    Released(Slot),
    /// Another key was pressed while a modifier-only hotkey was held (e.g. Ctrl+C).
    Interrupted(Slot),
    Escape,
}

pub type EventTx = UnboundedSender<HotkeyEvent>;

#[derive(Debug, Clone)]
pub struct Binding {
    pub slot: Slot,
    pub tokens: Vec<String>,
}

impl Binding {
    pub fn modifier_only(&self) -> bool {
        !self.tokens.is_empty() && self.tokens.iter().all(|t| is_modifier(t))
    }
}

pub const MODIFIERS: &[&str] = &[
    "Ctrl", "Shift", "Alt", "Super", "LeftCtrl", "RightCtrl", "LeftShift", "RightShift", "LeftAlt", "RightAlt",
    "LeftSuper", "RightSuper",
];

pub fn is_modifier(t: &str) -> bool {
    MODIFIERS.contains(&t)
}

/// Parses "Ctrl+Alt+Space" / "RightCtrl" into canonical tokens.
pub fn parse(accel: &str) -> Vec<String> {
    accel
        .split('+')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| {
            let canon = match s.to_ascii_lowercase().as_str() {
                "ctrl" | "control" | "cmdorctrl" | "commandorcontrol" => "Ctrl",
                "shift" => "Shift",
                "alt" | "option" => "Alt",
                "super" | "meta" | "cmd" | "command" | "win" | "logo" => "Super",
                "leftctrl" | "lctrl" | "controlleft" => "LeftCtrl",
                "rightctrl" | "rctrl" | "controlright" => "RightCtrl",
                "leftshift" | "lshift" | "shiftleft" => "LeftShift",
                "rightshift" | "rshift" | "shiftright" => "RightShift",
                "leftalt" | "lalt" | "altleft" => "LeftAlt",
                "rightalt" | "ralt" | "altgr" | "altright" => "RightAlt",
                "leftsuper" | "lsuper" | "metaleft" => "LeftSuper",
                "rightsuper" | "rsuper" | "metaright" => "RightSuper",
                "space" => "Space",
                "esc" | "escape" => "Escape",
                "enter" | "return" => "Enter",
                _ => "",
            };
            if canon.is_empty() {
                if s.len() == 1 {
                    s.to_ascii_uppercase()
                } else {
                    s.to_string()
                }
            } else {
                canon.to_string()
            }
        })
        .collect()
}

/// Builds the bindings for the current settings, merging identical keys into [`Slot::Shared`].
pub fn bindings(dictate: &str, command: &str, command_enabled: bool) -> Vec<Binding> {
    let d = parse(dictate);
    let c = parse(command);
    let same = {
        let mut a = d.clone();
        let mut b = c.clone();
        a.sort();
        b.sort();
        a == b
    };
    let mut out = Vec::new();
    if command_enabled && same && !d.is_empty() {
        out.push(Binding { slot: Slot::Shared, tokens: d });
        return out;
    }
    if !d.is_empty() {
        out.push(Binding { slot: Slot::Dictate, tokens: d });
    }
    if command_enabled && !c.is_empty() {
        out.push(Binding { slot: Slot::Command, tokens: c });
    }
    out
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct HotkeyStatus {
    /// Backend actually in use: "evdev", "portal", "native" or "none".
    pub backend: String,
    pub ok: bool,
    pub message: String,
    /// Human-readable triggers reported by the portal, per slot.
    pub portal_triggers: Vec<(String, String)>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_accelerators() {
        assert_eq!(parse("ctrl+alt+space"), vec!["Ctrl", "Alt", "Space"]);
        assert_eq!(parse("RightCtrl"), vec!["RightCtrl"]);
        assert_eq!(parse("Super+d"), vec!["Super", "D"]);
    }

    #[test]
    fn merges_shared_keys() {
        let b = bindings("RightCtrl", "RightCtrl", true);
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].slot, Slot::Shared);
        let b = bindings("Ctrl+Alt+D", "Alt+Ctrl+D", false);
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].slot, Slot::Dictate);
        let b = bindings("RightCtrl", "RightAlt", true);
        assert_eq!(b.len(), 2);
        assert!(b[0].modifier_only());
    }
}
