//! Tauri commands exposed to the settings UI.

use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::{
    audio::{self, Recorder, Sound},
    engine::{Engine, Input, Status, Trigger},
    history::{Entry, Stats},
    hotkeys::{manager::HotkeyManager, EventTx, HotkeyStatus},
    platform::{self, Environment},
    providers::{self, Endpoint, ModelInfo, ProviderInfo},
    settings::{ProviderConfig, Settings},
    tray,
};

pub struct AppState {
    pub settings_path: PathBuf,
    pub hotkeys: HotkeyManager,
    pub hotkey_tx: EventTx,
    pub mic_test: Arc<Recorder>,
    pub mic_test_on: Arc<AtomicBool>,
    pub mic_test_lock: tokio::sync::Mutex<()>,
}

type R<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[tauri::command]
pub fn get_settings(engine: State<'_, Arc<Engine>>) -> Settings {
    engine.settings.read().clone()
}

/// Re-applies everything that depends on settings (hotkeys, tray menu, overlay).
pub fn apply_settings(app: &AppHandle, engine: &Engine, state: &AppState) {
    let settings = engine.settings.read().clone();
    let modifier_only = state.hotkeys.apply(app, &settings, state.hotkey_tx.clone());
    *engine.modifier_only.write() = modifier_only;
    tray::refresh_menu(app, &settings);
}

#[tauri::command]
pub fn save_settings(app: AppHandle, engine: State<'_, Arc<Engine>>, state: State<'_, AppState>, settings: Settings) -> R<()> {
    let old = engine.settings.read().clone();
    settings.save(&state.settings_path).map_err(err)?;
    let hotkeys_changed = serde_json::to_string(&old.hotkeys).ok() != serde_json::to_string(&settings.hotkeys).ok()
        || old.command.enabled != settings.command.enabled;
    let menu_changed = serde_json::to_string(&old.command.quick_actions).ok()
        != serde_json::to_string(&settings.command.quick_actions).ok()
        || old.command.enabled != settings.command.enabled;
    *engine.settings.write() = settings.clone();
    if hotkeys_changed {
        let modifier_only = state.hotkeys.apply(&app, &settings, state.hotkey_tx.clone());
        *engine.modifier_only.write() = modifier_only;
    }
    if menu_changed {
        tray::refresh_menu(&app, &settings);
    }
    let _ = app.emit("od://settings", &settings);
    Ok(())
}

#[tauri::command]
pub fn reset_settings(app: AppHandle, engine: State<'_, Arc<Engine>>, state: State<'_, AppState>) -> R<Settings> {
    let old = engine.settings.read().clone();
    // Keep keys and onboarding state; reset behaviour.
    let fresh = Settings { providers: old.providers, onboarded: old.onboarded, ..Settings::default() };
    fresh.save(&state.settings_path).map_err(err)?;
    *engine.settings.write() = fresh.clone();
    apply_settings(&app, &engine, &state);
    let _ = app.emit("od://settings", &fresh);
    Ok(fresh)
}

#[tauri::command]
pub fn default_settings() -> Settings {
    Settings::default()
}

#[derive(Serialize)]
pub struct DefaultPrompts {
    pub polish: &'static str,
    pub selection: &'static str,
    pub generate: &'static str,
}

#[tauri::command]
pub fn default_prompts() -> DefaultPrompts {
    use crate::settings::*;
    DefaultPrompts {
        polish: DEFAULT_POLISH_PROMPT,
        selection: DEFAULT_COMMAND_PROMPT_SELECTION,
        generate: DEFAULT_COMMAND_PROMPT_GENERATE,
    }
}

#[tauri::command]
pub fn providers_info() -> Vec<ProviderInfo> {
    providers::PROVIDERS.to_vec()
}

#[tauri::command]
pub async fn list_models(engine: State<'_, Arc<Engine>>, provider: String, kind: String) -> R<Vec<ModelInfo>> {
    let ep = {
        let s = engine.settings.read();
        Endpoint::resolve(&s.providers, &provider).map_err(err)?
    };
    providers::list_models(engine.http(), &ep, &kind).await.map_err(|e| format!("{e:#}"))
}

/// Tests credentials that may not be saved yet.
#[tauri::command]
pub async fn test_provider(engine: State<'_, Arc<Engine>>, provider: String, api_key: String, base_url: String) -> R<usize> {
    let mut providers_cfg = engine.settings.read().providers.clone();
    let cfg = ProviderConfig { api_key, base_url };
    match provider.as_str() {
        "openrouter" => providers_cfg.openrouter = cfg,
        "openai" => providers_cfg.openai = cfg,
        "groq" => providers_cfg.groq = cfg,
        "mistral" => providers_cfg.mistral = cfg,
        "gemini" => providers_cfg.gemini = cfg,
        "anthropic" => providers_cfg.anthropic = cfg,
        "custom" => providers_cfg.custom = cfg,
        _ => return Err("unknown provider".into()),
    }
    let ep = Endpoint::resolve(&providers_cfg, &provider).map_err(err)?;
    providers::test_provider(engine.http(), &ep).await.map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub async fn input_devices() -> Vec<audio::InputDevice> {
    tauri::async_runtime::spawn_blocking(audio::list_input_devices).await.unwrap_or_default()
}

#[derive(Serialize)]
pub struct SystemStatus {
    pub version: &'static str,
    pub environment: Environment,
    pub hotkeys: HotkeyStatus,
    pub injection_ok: bool,
    pub injection_error: Option<String>,
}

#[tauri::command]
pub fn system_status(engine: State<'_, Arc<Engine>>, state: State<'_, AppState>) -> SystemStatus {
    let injection_ok = engine.output.can_inject();
    SystemStatus {
        version: env!("CARGO_PKG_VERSION"),
        environment: platform::environment(),
        hotkeys: state.hotkeys.status(),
        injection_ok,
        injection_error: if injection_ok { None } else { engine.output.injection_error() },
    }
}

#[tauri::command]
pub fn reapply_hotkeys(app: AppHandle, engine: State<'_, Arc<Engine>>, state: State<'_, AppState>) -> HotkeyStatus {
    apply_settings(&app, &engine, &state);
    state.hotkeys.status()
}

#[tauri::command]
pub fn history_list(engine: State<'_, Arc<Engine>>) -> Vec<Entry> {
    engine.history.list()
}

#[tauri::command]
pub fn history_delete(engine: State<'_, Arc<Engine>>, id: u64) {
    engine.history.delete(id);
}

#[tauri::command]
pub fn history_clear(engine: State<'_, Arc<Engine>>) {
    engine.history.clear();
}

#[tauri::command]
pub fn stats(engine: State<'_, Arc<Engine>>) -> Stats {
    engine.history.stats()
}

#[tauri::command]
pub fn last_status(window: tauri::WebviewWindow, engine: State<'_, Arc<Engine>>) -> Option<Status> {
    log::debug!("last_status requested by '{}'", window.label());
    engine.last_status()
}

#[tauri::command]
pub fn trigger(engine: State<'_, Arc<Engine>>, action: String, arg: Option<String>) -> R<()> {
    log::info!("trigger from the UI: {action}");
    let t = match action.as_str() {
        "dictate" => Trigger::ToggleDictate,
        "command" => Trigger::ToggleCommand,
        "stop" => Trigger::Stop,
        "cancel" => Trigger::Cancel,
        "retry" => Trigger::Retry,
        "action" => Trigger::QuickAction(arg.ok_or("missing action id")?),
        _ => return Err(format!("unknown action '{action}'")),
    };
    engine.send(Input::Trigger(t));
    Ok(())
}

#[tauri::command]
pub fn copy_text(engine: State<'_, Arc<Engine>>, text: String) -> R<()> {
    engine.output.set_clipboard(&text).map_err(err)
}

#[tauri::command]
pub fn play_sound(engine: State<'_, Arc<Engine>>, name: String) {
    let vol = engine.settings.read().audio.sound_volume;
    let s = match name.as_str() {
        "command" => Sound::CommandStart,
        "stop" => Sound::Stop,
        "done" => Sound::Done,
        "error" => Sound::Error,
        _ => Sound::Start,
    };
    audio::play(s, vol);
}

/// Starts/stops a microphone level test (emits `od://mic-level`). Async, and serialised, so that
/// opening a slow device (or one waiting for a permission prompt) never freezes the window and a
/// stop can't overtake the start before it.
#[tauri::command]
pub async fn mic_test(app: AppHandle, engine: State<'_, Arc<Engine>>, state: State<'_, AppState>, start: bool, device: Option<String>) -> R<()> {
    let _turn = state.mic_test_lock.lock().await;
    let rec = state.mic_test.clone();
    let on = state.mic_test_on.clone();
    if !start {
        on.store(false, Ordering::Relaxed);
        tauri::async_runtime::spawn_blocking(move || rec.cancel()).await.map_err(err)?;
        return Ok(());
    }
    if engine.recorder().is_recording() {
        return Err("The microphone is busy with a dictation".into());
    }
    #[cfg(target_os = "macos")]
    if crate::audio::test_audio().is_none() {
        match crate::macos::permissions::microphone_status() {
            "granted" => {}
            "undetermined" => {
                // Don't open the device while macOS asks: it would only deliver silence.
                crate::macos::permissions::request_microphone();
                return Err("Allow OpenDictate to use the microphone in the dialog.".into());
            }
            _ => return Err("Microphone access is off for OpenDictate. Allow it in System Settings → Privacy & Security → Microphone.".into()),
        }
    }
    let device = device.unwrap_or_else(|| engine.settings.read().audio.input_device.clone());
    {
        let rec = rec.clone();
        tauri::async_runtime::spawn_blocking(move || {
            rec.cancel();
            rec.start(&device)
        })
        .await
        .map_err(err)?
        .map_err(|e| format!("{e:#}"))?;
    }
    on.store(true, Ordering::Relaxed);
    tauri::async_runtime::spawn(async move {
        let mut ticks = 0;
        while on.load(Ordering::Relaxed) && ticks < 30 * 60 {
            tokio::time::sleep(Duration::from_millis(33)).await;
            let _ = app.emit("od://mic-level", rec.current_level());
            ticks += 1;
        }
        if on.swap(false, Ordering::Relaxed) {
            // Timed out (not stopped by the UI).
            tauri::async_runtime::spawn_blocking(move || rec.cancel());
        }
    });
    Ok(())
}

#[tauri::command]
pub async fn fix_permissions(app: AppHandle, input_group: bool) -> R<String> {
    let msg = tauri::async_runtime::spawn_blocking(move || platform::fix_permissions(input_group))
        .await
        .map_err(err)?
        .map_err(|e| format!("{e:#}"))?;
    // uinput access may be available right away.
    if let (Some(engine), Some(state)) = (app.try_state::<Arc<Engine>>(), app.try_state::<AppState>()) {
        tokio::time::sleep(Duration::from_millis(600)).await;
        let _ = engine.output.can_inject();
        apply_settings(&app, &engine, &state);
    }
    Ok(msg)
}

#[tauri::command]
pub fn show_window(app: AppHandle) {
    tray::show_main(&app, None);
}

#[tauri::command]
pub fn pause_hotkeys(app: AppHandle, engine: State<'_, Arc<Engine>>, state: State<'_, AppState>, paused: bool) {
    engine.paused.store(paused, Ordering::Relaxed);
    crate::hotkeys::PAUSED.store(paused, Ordering::Relaxed);
    // System hotkeys would swallow the very keys the user is about to record.
    #[cfg(not(target_os = "linux"))]
    {
        if paused {
            crate::hotkeys::native_backend::unregister_all(&app);
        } else {
            apply_settings(&app, &engine, &state);
        }
    }
    #[cfg(target_os = "linux")]
    let _ = (app, state);
}

/// macOS: shows the system prompt for `accessibility` or `microphone` access.
#[tauri::command]
pub fn request_permission(kind: String) -> R<()> {
    platform::request_permission(&kind).map_err(|e| format!("{e:#}"))
}

/// macOS: opens System Settings → Privacy & Security at `accessibility`, `microphone`, ...
#[tauri::command]
pub fn open_privacy_settings(pane: String) -> R<()> {
    platform::open_privacy_settings(&pane).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn open_shortcut_settings() -> R<()> {
    platform::open_shortcut_settings().map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn update_info(engine: State<'_, Arc<Engine>>) -> Option<crate::updates::UpdateInfo> {
    engine.update.lock().clone()
}

/// Checks GitHub for a newer release right now.
#[tauri::command]
pub async fn check_for_updates(engine: State<'_, Arc<Engine>>) -> R<Option<crate::updates::UpdateInfo>> {
    Ok(engine.check_updates().await)
}

/// Downloads, verifies and installs the latest release, then restarts.
#[tauri::command]
pub async fn install_update(app: AppHandle) -> R<()> {
    crate::updates::install(app).await.map_err(|e| format!("{e:#}"))
}
