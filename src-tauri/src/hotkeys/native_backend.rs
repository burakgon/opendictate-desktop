//! macOS / Windows global shortcuts through `tauri-plugin-global-shortcut`.

use anyhow::{anyhow, Result};
use tauri::AppHandle;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use super::{Binding, EventTx, HotkeyEvent};

fn to_accelerator(tokens: &[String]) -> Result<String> {
    let mut parts = Vec::new();
    let mut has_key = false;
    for t in tokens {
        let p = match t.as_str() {
            "Ctrl" | "LeftCtrl" | "RightCtrl" => "Control".to_string(),
            "Shift" | "LeftShift" | "RightShift" => "Shift".to_string(),
            "Alt" | "LeftAlt" | "RightAlt" => "Alt".to_string(),
            "Super" | "LeftSuper" | "RightSuper" => "Super".to_string(),
            "`" => "Backquote".to_string(),
            "-" => "Minus".to_string(),
            "=" => "Equal".to_string(),
            "[" => "BracketLeft".to_string(),
            "]" => "BracketRight".to_string(),
            "\\" => "Backslash".to_string(),
            ";" => "Semicolon".to_string(),
            "'" => "Quote".to_string(),
            "," => "Comma".to_string(),
            "." => "Period".to_string(),
            "/" => "Slash".to_string(),
            other => {
                has_key = true;
                other.to_string()
            }
        };
        if !matches!(p.as_str(), "Control" | "Shift" | "Alt" | "Super") {
            has_key = true;
        }
        parts.push(p);
    }
    if !has_key {
        return Err(anyhow!("modifier-only shortcuts are not supported on this platform - add a regular key"));
    }
    Ok(parts.join("+"))
}

pub fn apply(app: &AppHandle, bindings: &[Binding], tx: EventTx) -> Result<()> {
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    let mut errors = Vec::new();
    for b in bindings {
        let accel = match to_accelerator(&b.tokens) {
            Ok(a) => a,
            Err(e) => {
                errors.push(format!("{}: {e}", b.slot.id()));
                continue;
            }
        };
        let shortcut: Shortcut = match accel.parse() {
            Ok(s) => s,
            Err(e) => {
                errors.push(format!("{}: invalid shortcut '{accel}': {e}", b.slot.id()));
                continue;
            }
        };
        let slot = b.slot;
        let tx = tx.clone();
        if let Err(e) = gs.on_shortcut(shortcut, move |_app, _sc, event| {
            let ev = match event.state() {
                ShortcutState::Pressed => HotkeyEvent::Pressed(slot),
                ShortcutState::Released => HotkeyEvent::Released(slot),
            };
            let _ = tx.send(ev);
        }) {
            errors.push(format!("{}: {e}", b.slot.id()));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(anyhow!(errors.join("; ")))
    }
}
