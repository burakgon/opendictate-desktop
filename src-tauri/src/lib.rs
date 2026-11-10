//! OpenDictate - voice dictation and voice commands for the desktop.

mod active_app;
mod audio;
mod commands;
mod engine;
mod history;
mod hotkeys;
#[cfg(target_os = "macos")]
mod macos;
mod media;
mod notify;
mod output;
mod overlay;
mod platform;
mod providers;
mod settings;
mod tray;
mod updates;

use std::sync::{atomic::AtomicBool, Arc};

use parking_lot::RwLock;
use tauri::{AppHandle, Manager, RunEvent, WindowEvent};

use crate::{
    audio::Recorder,
    commands::AppState,
    engine::{Engine, Input, Trigger},
    history::History,
    hotkeys::{manager::HotkeyManager, HotkeyEvent},
    settings::Settings,
};

const USAGE: &str = "OpenDictate - voice dictation and voice commands for your desktop

Usage: opendictate [COMMAND]

Commands (sent to the running instance; it is started if needed):
  toggle | dictate     Start or stop dictation
  command              Start or stop a voice command
  stop                 Stop recording and process it
  cancel               Cancel recording / processing
  retry                Retry the last recording whose transcription failed
  action <id>          Apply a quick action to the selected text (e.g. `action fix`)
  show [page]          Open the main window (home, dictation, commands, providers, history, settings)
  update               Install the latest version (macOS, Windows, AppImage) and restart
  quit                 Quit OpenDictate

On macOS the same commands are available as URLs, e.g. `open opendictate://toggle`.

Options:
  --hidden             Start in the background without opening the window
  -h, --help           Show this help
  -V, --version        Show the version
";

/// Handles CLI arguments. Returns true if an action was triggered.
fn handle_args(app: &AppHandle, args: &[String]) -> bool {
    let Some(engine) = app.try_state::<Arc<Engine>>() else { return false };
    let mut it = args.iter().filter(|a| !a.starts_with("--"));
    let send = |t: Trigger| engine.send(Input::Trigger(t));
    match it.next().map(|s| s.as_str()) {
        Some("toggle") | Some("dictate") => send(Trigger::ToggleDictate),
        Some("command") => send(Trigger::ToggleCommand),
        Some("stop") => send(Trigger::Stop),
        Some("cancel") => send(Trigger::Cancel),
        Some("retry") => send(Trigger::Retry),
        Some("action") => match it.next() {
            Some(id) => send(Trigger::QuickAction(id.clone())),
            None => return false,
        },
        Some("show") | Some("settings") => tray::show_main(app, it.next().map(|s| s.as_str())),
        Some("update") => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = updates::install(app).await {
                    log::error!("update failed: {e:#}");
                    notify::send("OpenDictate".into(), format!("{e:#}"));
                }
            });
        }
        Some("quit") => app.exit(0),
        _ => return false,
    }
    true
}

pub fn run() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        print!("{USAGE}");
        return;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("opendictate {}", env!("CARGO_PKG_VERSION"));
        return;
    }

    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("opendictate_lib=info,warn"))
        .format_timestamp_millis()
        .init();

    // What a second launch asks for (`opendictate toggle`, or just opening the app again) is done
    // on a thread of its own, in order. Not inside the plugin's handler: on Windows that is a
    // window procedure answering the other process, and a window can't be created from there -
    // which would leave no way back to the window once the tray icon is hidden.
    let (launch_tx, launch_rx) = std::sync::mpsc::channel::<(AppHandle, Vec<String>)>();
    std::thread::spawn(move || {
        for (app, args) in launch_rx {
            if !handle_args(&app, &args) {
                tray::show_main(&app, None);
            }
        }
    });

    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(move |app, argv, _cwd| {
            let _ = launch_tx.send((app.clone(), argv.into_iter().skip(1).collect()));
        }))
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, Some(vec!["--hidden"])))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build());

    #[cfg(not(target_os = "linux"))]
    {
        builder = builder.plugin(tauri_plugin_global_shortcut::Builder::new().build());
    }
    // macOS: never grab the focus at launch. A menu bar app started in the background must not
    // activate itself; macOS would otherwise honour that request the first time the overlay
    // appears and the dictated text would land in OpenDictate instead of the user's app.
    // (Opening the window still activates it explicitly.)
    #[cfg(target_os = "macos")]
    {
        builder = builder.activate_ignoring_other_apps(false);
    }

    builder = builder.setup(move |app| {
        let handle = app.handle().clone();
        platform::ensure_desktop_entry();
        #[cfg(target_os = "macos")]
        {
            // A menu bar app: the Dock icon only appears while the window is open.
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            app.set_menu(tray::app_menu(&handle)?)?;
            macos::on_main(macos::keys::watch_layout);
        }
        let config_dir = app.path().app_config_dir()?;
        let data_dir = app.path().app_data_dir()?;
        let settings_path = config_dir.join("settings.json");
        let settings = Settings::load(&settings_path);
        log::info!("settings: {}", settings_path.display());

        let settings = Arc::new(RwLock::new(settings));
        let history = Arc::new(History::load(data_dir.join("history.json")));
        let (engine, rx) = Engine::new(handle.clone(), settings.clone(), history);
        app.manage(engine.clone());
        tauri::async_runtime::spawn(engine.clone().run(rx));

        // Hotkey backends talk to the engine through this channel.
        let (hk_tx, mut hk_rx) = tokio::sync::mpsc::unbounded_channel::<HotkeyEvent>();
        {
            let tx = engine.sender();
            tauri::async_runtime::spawn(async move {
                while let Some(ev) = hk_rx.recv().await {
                    let _ = tx.send(Input::Hotkey(ev));
                }
            });
        }

        let current = settings.read().clone();
        overlay::create(&handle, current.general.overlay_position)?;
        tray::sync(&handle, &current)?;

        app.manage(AppState {
            settings_path,
            hotkeys: HotkeyManager::new(),
            hotkey_tx: hk_tx,
            mic_test: Arc::new(Recorder::new()),
            mic_test_on: Arc::new(AtomicBool::new(false)),
            mic_test_lock: tokio::sync::Mutex::new(()),
        });
        {
            let state = app.state::<AppState>();
            commands::apply_settings(&handle, &engine, &state);
        }

        // Never hide the first-run setup, even when launched with --hidden at login.
        // Look for a newer release shortly after start and then once a day.
        if current.general.check_updates {
            let engine = engine.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(20)).await;
                loop {
                    if engine.settings.read().general.check_updates {
                        if let Err(e) = engine.check_updates().await {
                            log::info!("{e}");
                        }
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(24 * 3600)).await;
                }
            });
        }

        let hidden = current.onboarded && (args.iter().any(|a| a == "--hidden") || current.general.start_hidden);
        let acted = handle_args(&handle, &args);
        if !hidden && !acted {
            tray::show_main(&handle, None);
        }
        Ok(())
    });

    let app = builder
        // The tray menu and the macOS app menu. Registered here rather than on the tray icon,
        // which is optional: the app menu has to work without it.
        .on_menu_event(|app, event| tray::on_menu(app, event.id.as_ref()))
        .on_window_event(|window, event| {
            if window.label() == tray::MAIN_WINDOW {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    // Hide instead of destroying: WebKitGTK keeps a destroyed view's web process
                    // alive, so re-creating the window each time would leak memory. (On macOS it
                    // is released a little later, see `tray::main_window_closed`.)
                    api.prevent_close();
                    let _ = window.hide();
                    tray::main_window_closed(window.app_handle());
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::save_settings,
            commands::reset_settings,
            commands::default_settings,
            commands::default_prompts,
            commands::providers_info,
            commands::list_models,
            commands::test_provider,
            commands::input_devices,
            commands::system_status,
            commands::reapply_hotkeys,
            commands::history_list,
            commands::history_delete,
            commands::history_clear,
            commands::stats,
            commands::last_status,
            commands::trigger,
            commands::copy_text,
            commands::play_sound,
            commands::mic_test,
            commands::fix_permissions,
            commands::show_window,
            commands::quit,
            commands::pause_hotkeys,
            commands::open_shortcut_settings,
            commands::update_info,
            commands::request_permission,
            commands::open_privacy_settings,
            commands::check_for_updates,
            commands::install_update,
        ])
        .build(tauri::generate_context!())
        .expect("error while building OpenDictate");

    app.run(|app, event| match event {
        RunEvent::ExitRequested { api, code, .. } => {
            if code.is_none() {
                api.prevent_exit();
            }
        }
        // Clicking the Dock icon or opening the app again from Finder / Spotlight.
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { .. } => tray::show_main(app, None),
        // opendictate://toggle, opendictate://action/fix, opendictate://show/settings, ...
        #[cfg(target_os = "macos")]
        RunEvent::Opened { urls } => {
            for url in urls.iter().filter(|u| u.scheme() == "opendictate") {
                let mut args: Vec<String> = url.host_str().map(|h| vec![h.to_string()]).unwrap_or_default();
                args.extend(url.path_segments().into_iter().flatten().filter(|s| !s.is_empty()).map(str::to_string));
                log::info!("url: {url}");
                if !handle_args(app, &args) {
                    tray::show_main(app, None);
                }
            }
        }
        _ => {
            let _ = app;
        }
    });
}
