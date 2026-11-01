//! macOS / Windows global shortcuts through `tauri-plugin-global-shortcut` (Carbon hotkeys on
//! macOS, `RegisterHotKey` on Windows).

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
            "Fn" => return Err(anyhow!("the fn key can't be part of a system shortcut")),
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
            other => other.to_string(),
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

/// Registers `bindings` (replacing all previous ones). Returns the bindings that could not be
/// registered, with the reason.
pub fn apply(app: &AppHandle, bindings: &[Binding], tx: EventTx) -> Vec<(Binding, String)> {
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    let mut failed = Vec::new();
    for b in bindings {
        let shortcut: Shortcut = match to_accelerator(&b.tokens).and_then(|a| a.parse::<Shortcut>().map_err(|e| anyhow!("invalid shortcut '{a}': {e}"))) {
            Ok(s) => s,
            Err(e) => {
                failed.push((b.clone(), format!("{e}")));
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
            failed.push((b.clone(), format!("{e}")));
        }
    }
    failed
}

/// Esc cancels a recording. It is grabbed only while OpenDictate is busy, so Escape keeps working
/// normally in every app the rest of the time (and isn't sent to the app while it cancels).
pub fn set_escape(app: &AppHandle, enabled: bool, on_escape: impl Fn() + Send + Sync + 'static) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        let gs = handle.global_shortcut();
        let escape: Shortcut = match "Escape".parse() {
            Ok(s) => s,
            Err(_) => return,
        };
        if enabled {
            if gs.is_registered(escape) {
                return;
            }
            let result = gs.on_shortcut(escape, move |_app, _sc, event| {
                if event.state() == ShortcutState::Pressed {
                    on_escape();
                }
            });
            if let Err(e) = result {
                log::debug!("could not grab Escape: {e}");
            }
        } else if gs.is_registered(escape) {
            let _ = gs.unregister(escape);
        }
    });
}

pub fn unregister_all(app: &AppHandle) {
    let _ = app.global_shortcut().unregister_all();
}
