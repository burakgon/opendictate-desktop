//! Linux hotkeys via the XDG desktop portal `GlobalShortcuts` interface.
//!
//! Works without special permissions on KDE Plasma, GNOME 48+ and Hyprland. The desktop asks
//! the user to confirm the shortcuts the first time and lets them change the keys later.

use std::sync::Arc;

use anyhow::{Context, Result};
use ashpd::desktop::{
    global_shortcuts::{BindShortcutsOptions, GlobalShortcuts, NewShortcut},
    CreateSessionOptions,
};
use futures_util::StreamExt;
use parking_lot::Mutex;

use super::{Binding, EventTx, HotkeyEvent, HotkeyStatus, Slot};

pub const APP_ID: &str = "io.github.burakgon.OpenDictate";

/// Converts canonical tokens to the XDG "shortcuts" trigger syntax (e.g. `CTRL+ALT+space`).
pub fn to_trigger(tokens: &[String]) -> Option<String> {
    let mut mods = Vec::new();
    let mut key = None;
    for t in tokens {
        match t.as_str() {
            "Ctrl" | "LeftCtrl" | "RightCtrl" => mods.push("CTRL"),
            "Shift" | "LeftShift" | "RightShift" => mods.push("SHIFT"),
            "Alt" | "LeftAlt" | "RightAlt" => mods.push("ALT"),
            "Super" | "LeftSuper" | "RightSuper" => mods.push("LOGO"),
            other => {
                let k = match other {
                    "Space" => "space".to_string(),
                    "Enter" => "Return".to_string(),
                    "Escape" => "Escape".to_string(),
                    "Backspace" => "BackSpace".to_string(),
                    "PageUp" => "Page_Up".to_string(),
                    "PageDown" => "Page_Down".to_string(),
                    "PrintScreen" => "Print".to_string(),
                    "`" => "grave".to_string(),
                    s if s.len() == 1 => s.to_ascii_lowercase(),
                    s => s.to_string(),
                };
                key = Some(k);
            }
        }
    }
    let key = key?;
    mods.dedup();
    mods.push(&key);
    Some(mods.join("+"))
}

fn description(slot: Slot) -> &'static str {
    match slot {
        Slot::Dictate => "Dictate: start / stop speech to text",
        Slot::Command => "Voice command: speak an instruction for the selected text",
        Slot::Shared => "Dictate (tap) or voice command (hold)",
    }
}

fn slot_from_id(id: &str) -> Option<Slot> {
    match id {
        "dictate" => Some(Slot::Dictate),
        "command" => Some(Slot::Command),
        "shared" => Some(Slot::Shared),
        _ => None,
    }
}

pub struct PortalHandle {
    task: tauri::async_runtime::JoinHandle<()>,
}

impl Drop for PortalHandle {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub fn start(bindings: Vec<Binding>, tx: EventTx, status: Arc<Mutex<HotkeyStatus>>) -> PortalHandle {
    let task = tauri::async_runtime::spawn(async move {
        if let Err(e) = run(bindings, tx, status.clone()).await {
            log::warn!("global shortcuts portal failed: {e:#}");
            let mut s = status.lock();
            s.ok = false;
            s.message = format!("Desktop shortcut portal unavailable: {e:#}");
        }
    });
    PortalHandle { task }
}

async fn run(bindings: Vec<Binding>, tx: EventTx, status: Arc<Mutex<HotkeyStatus>>) -> Result<()> {
    if let Ok(app_id) = ashpd::AppID::try_from(APP_ID) {
        if let Err(e) = ashpd::register_host_app(app_id).await {
            log::debug!("portal host registration: {e}");
        }
    }
    let portal = GlobalShortcuts::new().await.context("GlobalShortcuts portal not available")?;
    let session = portal.create_session(CreateSessionOptions::default()).await.context("could not create a shortcut session")?;
    let shortcuts: Vec<NewShortcut> = bindings
        .iter()
        .map(|b| {
            let trigger = to_trigger(&b.tokens);
            NewShortcut::new(b.slot.id(), description(b.slot)).preferred_trigger(trigger.as_deref())
        })
        .collect();
    let request = portal
        .bind_shortcuts(&session, &shortcuts, None, BindShortcutsOptions::default())
        .await
        .context("binding shortcuts failed")?;
    let bound = request.response().context("the shortcut request was declined")?;
    {
        let mut s = status.lock();
        s.ok = true;
        s.message = "Shortcuts registered with the desktop. Change them in your system shortcut settings.".into();
        s.portal_triggers = bound
            .shortcuts()
            .iter()
            .map(|sc| (sc.id().to_string(), sc.trigger_description().to_string()))
            .collect();
    }
    log::info!(
        "portal shortcuts bound: {:?}",
        bound.shortcuts().iter().map(|s| (s.id(), s.trigger_description())).collect::<Vec<_>>()
    );

    let mut activated = portal.receive_activated().await?;
    let mut deactivated = portal.receive_deactivated().await?;
    loop {
        tokio::select! {
            Some(a) = activated.next() => {
                if let Some(slot) = slot_from_id(a.shortcut_id()) {
                    let _ = tx.send(HotkeyEvent::Pressed(slot));
                }
            }
            Some(d) = deactivated.next() => {
                if let Some(slot) = slot_from_id(d.shortcut_id()) {
                    let _ = tx.send(HotkeyEvent::Released(slot));
                }
            }
            else => break,
        }
    }
    // Keep the session alive for as long as the loop runs.
    drop(session);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hotkeys::parse;

    #[test]
    fn triggers() {
        assert_eq!(to_trigger(&parse("Ctrl+Alt+Space")).as_deref(), Some("CTRL+ALT+space"));
        assert_eq!(to_trigger(&parse("Super+D")).as_deref(), Some("LOGO+d"));
        assert_eq!(to_trigger(&parse("RightCtrl")), None);
        assert_eq!(to_trigger(&parse("F9")).as_deref(), Some("F9"));
    }
}
