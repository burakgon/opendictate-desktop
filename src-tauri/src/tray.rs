//! System tray icon and menu.

use tauri::{
    image::Image,
    menu::{Menu, MenuItem, PredefinedMenuItem, Submenu},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, Wry,
};

use crate::{
    engine::{Input, Trigger},
    settings::Settings,
};

const TRAY_ID: &str = "main";

fn icon(active: bool) -> Option<Image<'static>> {
    let bytes: &[u8] = if active { include_bytes!("../icons/tray-active.png") } else { include_bytes!("../icons/tray.png") };
    Image::from_bytes(bytes).ok()
}

fn build_menu(app: &AppHandle, settings: &Settings) -> tauri::Result<Menu<Wry>> {
    let dictate = MenuItem::with_id(app, "dictate", "Start / Stop Dictation", true, None::<&str>)?;
    let command = MenuItem::with_id(app, "command", "Voice Command", settings.command.enabled, None::<&str>)?;
    let cancel = MenuItem::with_id(app, "cancel", "Cancel", true, None::<&str>)?;
    let actions = Submenu::with_id(app, "actions", "Apply to Selection", !settings.command.quick_actions.is_empty())?;
    for a in &settings.command.quick_actions {
        actions.append(&MenuItem::with_id(app, format!("action:{}", a.id), &a.name, true, None::<&str>)?)?;
    }
    let open = MenuItem::with_id(app, "open", "Open OpenDictate", true, None::<&str>)?;
    let history = MenuItem::with_id(app, "history", "History", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit OpenDictate", true, None::<&str>)?;
    Menu::with_items(
        app,
        &[
            &dictate,
            &command,
            &cancel,
            &PredefinedMenuItem::separator(app)?,
            &actions,
            &PredefinedMenuItem::separator(app)?,
            &open,
            &history,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )
}

pub fn show_main(app: &AppHandle, route: Option<&str>) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        if let Some(r) = route {
            use tauri::Emitter;
            let _ = w.emit("od://navigate", r);
        }
    }
}

pub fn create(app: &AppHandle, settings: &Settings) -> tauri::Result<()> {
    let menu = build_menu(app, settings)?;
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("OpenDictate")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            let send = |t: Trigger| {
                if let Some(engine) = app.try_state::<std::sync::Arc<crate::engine::Engine>>() {
                    engine.send(Input::Trigger(t));
                }
            };
            log::info!("tray menu: {}", event.id.as_ref());
            match event.id.as_ref() {
                "dictate" => send(Trigger::ToggleDictate),
                "command" => send(Trigger::ToggleCommand),
                "cancel" => send(Trigger::Cancel),
                "open" => show_main(app, None),
                "history" => show_main(app, Some("history")),
                "quit" => app.exit(0),
                id if id.starts_with("action:") => send(Trigger::QuickAction(id["action:".len()..].to_string())),
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_main(tray.app_handle(), None);
            }
        });
    if let Some(i) = icon(false) {
        builder = builder.icon(i);
    } else if let Some(i) = app.default_window_icon() {
        builder = builder.icon(i.clone());
    }
    builder.build(app)?;
    Ok(())
}

pub fn refresh_menu(app: &AppHandle, settings: &Settings) {
    if let (Some(tray), Ok(menu)) = (app.tray_by_id(TRAY_ID), build_menu(app, settings)) {
        let _ = tray.set_menu(Some(menu));
    }
}

pub fn set_active(app: &AppHandle, active: bool) {
    let app2 = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(tray) = app2.tray_by_id(TRAY_ID) {
            let _ = tray.set_icon(icon(active));
            let _ = tray.set_tooltip(Some(if active { "OpenDictate - listening" } else { "OpenDictate" }));
        }
    });
}
