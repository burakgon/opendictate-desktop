//! OpenDictate - voice dictation and voice commands for the desktop.

mod active_app;
mod audio;
mod commands;
mod engine;
mod history;
mod hotkeys;
mod output;
mod overlay;
mod platform;
mod providers;
mod settings;
mod tray;

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
  action <id>          Apply a quick action to the selected text (e.g. `action fix`)
  show [page]          Open the main window (home, dictation, commands, providers, history, settings)
  quit                 Quit OpenDictate

Options:
  --hidden             Start in the tray without opening the window
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
        Some("action") => match it.next() {
            Some(id) => send(Trigger::QuickAction(id.clone())),
            None => return false,
        },
        Some("show") | Some("settings") => tray::show_main(app, it.next().map(|s| s.as_str())),
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

    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            let rest: Vec<String> = argv.into_iter().skip(1).collect();
            if !handle_args(app, &rest) {
                tray::show_main(app, None);
            }
        }))
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, Some(vec!["--hidden"])))
        .plugin(tauri_plugin_opener::init());

    #[cfg(not(target_os = "linux"))]
    {
        builder = builder.plugin(tauri_plugin_global_shortcut::Builder::new().build());
    }

    builder = builder.setup(move |app| {
        let handle = app.handle().clone();
        platform::ensure_desktop_entry();
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
        tray::create(&handle, &current)?;

        app.manage(AppState {
            settings_path,
            hotkeys: HotkeyManager::new(),
            hotkey_tx: hk_tx,
            mic_test: Arc::new(Recorder::new()),
            mic_test_on: Arc::new(AtomicBool::new(false)),
        });
        {
            let state = app.state::<AppState>();
            commands::apply_settings(&handle, &engine, &state);
        }

        let hidden = args.iter().any(|a| a == "--hidden") || (current.general.start_hidden && current.onboarded);
        let acted = handle_args(&handle, &args);
        if !hidden && !acted {
            tray::show_main(&handle, None);
        }
        Ok(())
    });

    let app = builder
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    // Keep running in the tray.
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::save_settings,
            commands::reset_settings,
            commands::default_settings,
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
            commands::pause_hotkeys,
        ])
        .build(tauri::generate_context!())
        .expect("error while building OpenDictate");

    app.run(|_app, event| {
        if let RunEvent::ExitRequested { api, code, .. } = event {
            if code.is_none() {
                api.prevent_exit();
            }
        }
    });
}
