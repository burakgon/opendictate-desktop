//! System tray (macOS: menu bar) icon and menu, the main window, and the macOS app menu.

#[cfg(not(target_os = "macos"))]
use tauri::tray::{MouseButton, MouseButtonState, TrayIconEvent};
use tauri::{
    image::Image,
    menu::{Menu, MenuItem, PredefinedMenuItem, Submenu},
    tray::TrayIconBuilder,
    AppHandle, Manager, Wry,
};

use crate::{
    engine::{Input, Trigger},
    settings::Settings,
};

const TRAY_ID: &str = "main";
pub const MAIN_WINDOW: &str = "main";

#[cfg(not(target_os = "macos"))]
fn icon(active: bool) -> Option<Image<'static>> {
    let bytes: &[u8] = if active { include_bytes!("../icons/tray-active.png") } else { include_bytes!("../icons/tray.png") };
    Image::from_bytes(bytes).ok()
}

/// macOS menu bar icons are monochrome templates that adapt to light and dark menu bars.
#[cfg(target_os = "macos")]
fn icon(active: bool) -> Option<Image<'static>> {
    let bytes: &[u8] =
        if active { include_bytes!("../icons/tray-template-active.png") } else { include_bytes!("../icons/tray-template.png") };
    Image::from_bytes(bytes).ok()
}

fn build_menu(app: &AppHandle, settings: &Settings) -> tauri::Result<Menu<Wry>> {
    let dictate = MenuItem::with_id(app, "dictate", "Start / Stop Dictation", true, None::<&str>)?;
    let command = MenuItem::with_id(app, "command", "Voice Command", settings.command.enabled, None::<&str>)?;
    let cancel = MenuItem::with_id(app, "cancel", "Cancel", true, None::<&str>)?;
    let retry = MenuItem::with_id(app, "retry", "Retry Last Recording", true, None::<&str>)?;
    let actions = Submenu::with_id(app, "actions", "Apply to Selection", !settings.command.quick_actions.is_empty())?;
    for a in &settings.command.quick_actions {
        actions.append(&MenuItem::with_id(app, format!("action:{}", a.id), &a.name, true, None::<&str>)?)?;
    }
    let open = MenuItem::with_id(app, "open", "Open OpenDictate", true, None::<&str>)?;
    let update = app
        .try_state::<std::sync::Arc<crate::engine::Engine>>()
        .and_then(|engine| engine.update.lock().clone())
        .map(|u| {
            let label = if u.can_install { format!("Update to {} and Restart", u.version) } else { format!("Download OpenDictate {}…", u.version) };
            MenuItem::with_id(app, if u.can_install { "update:install" } else { "update:page" }, label, true, None::<&str>)
        })
        .transpose()?;
    let history = MenuItem::with_id(app, "history", "History", true, None::<&str>)?;
    #[cfg(target_os = "macos")]
    let settings_item = MenuItem::with_id(app, "settings", "Settings…", true, Some("CmdOrCtrl+,"))?;
    let quit = MenuItem::with_id(app, "quit", "Quit OpenDictate", true, if cfg!(target_os = "macos") { Some("CmdOrCtrl+Q") } else { None })?;
    let separator = PredefinedMenuItem::separator(app)?;
    let mut top: Vec<&dyn tauri::menu::IsMenuItem<Wry>> = Vec::new();
    if let Some(u) = &update {
        top.push(u);
        top.push(&separator);
    }
    let menu = Menu::with_items(app, &top)?;
    menu.append_items(
        &[
            &dictate,
            &command,
            &cancel,
            &retry,
            &PredefinedMenuItem::separator(app)?,
            &actions,
            &PredefinedMenuItem::separator(app)?,
            &open,
            &history,
            #[cfg(target_os = "macos")]
            &settings_item,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;
    Ok(menu)
}

/// The menu bar of the app while its window is open (macOS).
#[cfg(target_os = "macos")]
pub fn app_menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    use tauri::menu::AboutMetadata;

    let info = app.package_info();
    let about = AboutMetadata {
        name: Some("OpenDictate".into()),
        version: Some(info.version.to_string()),
        copyright: app.config().bundle.copyright.clone(),
        website: Some("https://github.com/burakgon/opendictate-desktop".into()),
        website_label: Some("github.com/burakgon/opendictate-desktop".into()),
        ..Default::default()
    };
    let sep = || PredefinedMenuItem::separator(app);
    let app_menu = Submenu::with_items(
        app,
        "OpenDictate",
        true,
        &[
            &PredefinedMenuItem::about(app, Some("About OpenDictate"), Some(about))?,
            &sep()?,
            &MenuItem::with_id(app, "settings", "Settings…", true, Some("CmdOrCtrl+,"))?,
            &sep()?,
            &PredefinedMenuItem::services(app, None)?,
            &sep()?,
            &PredefinedMenuItem::hide(app, Some("Hide OpenDictate"))?,
            &PredefinedMenuItem::hide_others(app, None)?,
            &PredefinedMenuItem::show_all(app, None)?,
            &sep()?,
            &MenuItem::with_id(app, "quit", "Quit OpenDictate", true, Some("CmdOrCtrl+Q"))?,
        ],
    )?;
    let edit = Submenu::with_items(
        app,
        "Edit",
        true,
        &[
            &PredefinedMenuItem::undo(app, None)?,
            &PredefinedMenuItem::redo(app, None)?,
            &sep()?,
            &PredefinedMenuItem::cut(app, None)?,
            &PredefinedMenuItem::copy(app, None)?,
            &PredefinedMenuItem::paste(app, None)?,
            &PredefinedMenuItem::select_all(app, None)?,
        ],
    )?;
    let view = Submenu::with_items(
        app,
        "View",
        true,
        &[
            &MenuItem::with_id(app, "nav:home", "Home", true, Some("CmdOrCtrl+1"))?,
            &MenuItem::with_id(app, "nav:dictation", "Dictation", true, Some("CmdOrCtrl+2"))?,
            &MenuItem::with_id(app, "nav:commands", "Commands", true, Some("CmdOrCtrl+3"))?,
            &MenuItem::with_id(app, "nav:providers", "Providers", true, Some("CmdOrCtrl+4"))?,
            &MenuItem::with_id(app, "nav:history", "History", true, Some("CmdOrCtrl+5"))?,
        ],
    )?;
    let window = Submenu::with_items(
        app,
        "Window",
        true,
        &[
            &PredefinedMenuItem::minimize(app, None)?,
            &PredefinedMenuItem::maximize(app, None)?,
            &sep()?,
            &PredefinedMenuItem::close_window(app, None)?,
        ],
    )?;
    let help = Submenu::with_items(
        app,
        "Help",
        true,
        &[
            &MenuItem::with_id(app, "help", "OpenDictate Help", true, None::<&str>)?,
            &MenuItem::with_id(app, "issue", "Report an Issue…", true, None::<&str>)?,
        ],
    )?;
    Menu::with_items(app, &[&app_menu, &edit, &view, &window, &help])
}

/// Handles menu items from the tray menu and the macOS app menu.
pub fn on_menu(app: &AppHandle, id: &str) {
    let send = |t: Trigger| {
        if let Some(engine) = app.try_state::<std::sync::Arc<crate::engine::Engine>>() {
            engine.send(Input::Trigger(t));
        }
    };
    log::info!("menu: {id}");
    match id {
        "dictate" => send(Trigger::ToggleDictate),
        "command" => send(Trigger::ToggleCommand),
        "cancel" => send(Trigger::Cancel),
        "retry" => send(Trigger::Retry),
        "open" => show_main(app, None),
        "history" => show_main(app, Some("history")),
        "settings" => show_main(app, Some("settings")),
        "quit" => app.exit(0),
        "update:install" => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = crate::updates::install(app.clone()).await {
                    log::error!("update failed: {e:#}");
                    crate::notify::send("OpenDictate".into(), format!("The update failed: {e:#}"));
                }
            });
        }
        "update:page" => {
            let url = app.try_state::<std::sync::Arc<crate::engine::Engine>>().and_then(|e| e.update.lock().as_ref().map(|u| u.url.clone()));
            open_url(app, url.as_deref().unwrap_or("https://github.com/burakgon/opendictate-desktop/releases/latest"));
        }
        "help" => open_url(app, "https://github.com/burakgon/opendictate-desktop#readme"),
        "issue" => open_url(app, "https://github.com/burakgon/opendictate-desktop/issues/new/choose"),
        id if id.starts_with("nav:") => show_main(app, Some(&id[4..])),
        id if id.starts_with("action:") => send(Trigger::QuickAction(id["action:".len()..].to_string())),
        _ => {}
    }
}

fn open_url(app: &AppHandle, url: &str) {
    use tauri_plugin_opener::OpenerExt;
    let _ = app.opener().open_url(url, None::<&str>);
}

/// Shows the main window, creating it on first use: when OpenDictate starts hidden in the tray
/// (e.g. at login) no web view is loaded until the window is actually opened.
pub fn show_main(app: &AppHandle, route: Option<&str>) {
    // A window means a Dock icon and a menu bar; without one OpenDictate lives in the menu bar only.
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);

    if let Some(w) = app.get_webview_window(MAIN_WINDOW) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        if let Some(r) = route {
            use tauri::Emitter;
            let _ = w.emit("od://navigate", r);
        }
        return;
    }
    let url = match route {
        Some(r) => format!("index.html#{r}"),
        None => "index.html".to_string(),
    };
    let builder = tauri::WebviewWindowBuilder::new(app, MAIN_WINDOW, tauri::WebviewUrl::App(url.into()))
        .title("OpenDictate")
        .inner_size(1120.0, 760.0)
        .min_inner_size(920.0, 620.0)
        .center()
        .resizable(true)
        .visible(true);
    // macOS: a regular window with the native traffic lights over the app's own sidebar,
    // native corners and shadow; opaque, so no private API is needed.
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true)
        .traffic_light_position(tauri::LogicalPosition::new(20.0, 22.0))
        .background_color(tauri::window::Color(11, 11, 19, 255));
    #[cfg(not(target_os = "macos"))]
    let builder = builder.decorations(false).transparent(true).shadow(true);
    match builder.build() {
        Ok(w) => {
            let _ = w.set_focus();
        }
        Err(e) => log::error!("could not open the main window: {e}"),
    }
}

/// Called when the main window is closed (it is hidden, not destroyed, right away).
pub fn main_window_closed(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    {
        // Back to a menu bar app: no Dock icon, and hand the focus to the app the user was in.
        let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
        let _ = app.hide();
        // Release the web view (and its ~100 MB content process) once pending settings are saved.
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            if let Some(w) = app.get_webview_window(MAIN_WINDOW) {
                if !w.is_visible().unwrap_or(true) {
                    log::debug!("releasing the main window");
                    let _ = w.destroy();
                }
            }
        });
    }
    #[cfg(not(target_os = "macos"))]
    let _ = app;
}

pub fn create(app: &AppHandle, settings: &Settings) -> tauri::Result<()> {
    let menu = build_menu(app, settings)?;
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("OpenDictate")
        .menu(&menu)
        .on_menu_event(|app, event| on_menu(app, event.id.as_ref()));
    // macOS convention: clicking a menu bar item opens its menu. Elsewhere a click opens the window.
    #[cfg(target_os = "macos")]
    {
        builder = builder.show_menu_on_left_click(true).icon_as_template(true);
    }
    #[cfg(not(target_os = "macos"))]
    {
        builder = builder.show_menu_on_left_click(false).on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_main(tray.app_handle(), None);
            }
        });
    }
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
            // Setting the icon resets the template flag.
            #[cfg(target_os = "macos")]
            let _ = tray.set_icon_as_template(true);
            let _ = tray.set_tooltip(Some(if active { "OpenDictate - listening" } else { "OpenDictate" }));
        }
    });
}
