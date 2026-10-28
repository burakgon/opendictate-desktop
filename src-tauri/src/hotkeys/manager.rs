//! Chooses and (re)configures the hotkey backend for the current settings.

use std::{collections::HashMap, sync::Arc};

use parking_lot::Mutex;
use tauri::AppHandle;

use super::{bindings, Binding, EventTx, HotkeyStatus, Slot};
use crate::settings::{HotkeyBackend, Settings};

pub struct HotkeyManager {
    status: Arc<Mutex<HotkeyStatus>>,
    #[cfg(target_os = "linux")]
    evdev: Mutex<Option<Arc<super::evdev_backend::EvdevListener>>>,
    #[cfg(target_os = "linux")]
    portal: Mutex<Option<(Vec<String>, super::portal_backend::PortalHandle)>>,
}

impl HotkeyManager {
    pub fn new() -> Self {
        Self {
            status: Arc::new(Mutex::new(HotkeyStatus::default())),
            #[cfg(target_os = "linux")]
            evdev: Mutex::new(None),
            #[cfg(target_os = "linux")]
            portal: Mutex::new(None),
        }
    }

    pub fn status(&self) -> HotkeyStatus {
        self.status.lock().clone()
    }

    /// Applies the hotkey settings. Returns which bindings are modifier-only.
    pub fn apply(&self, app: &AppHandle, settings: &Settings, tx: EventTx) -> HashMap<Slot, bool> {
        let hk = &settings.hotkeys;
        let binds = bindings(&hk.dictate, &hk.command, settings.command.enabled);
        let modifier_only = binds.iter().map(|b| (b.slot, b.modifier_only())).collect();
        let _ = app;
        #[cfg(target_os = "linux")]
        self.apply_linux(hk.backend, binds, tx);
        #[cfg(not(target_os = "linux"))]
        self.apply_native(app, hk.backend, binds, tx);
        log::info!("hotkeys: {:?}", self.status.lock());
        modifier_only
    }

    #[cfg(target_os = "linux")]
    fn apply_linux(&self, backend: HotkeyBackend, binds: Vec<Binding>, tx: EventTx) {
        use super::{evdev_backend, portal_backend};

        let want = match backend {
            HotkeyBackend::Auto => {
                if evdev_backend::can_read_input_devices() {
                    HotkeyBackend::Evdev
                } else {
                    HotkeyBackend::Portal
                }
            }
            HotkeyBackend::Native => HotkeyBackend::Portal,
            other => other,
        };

        // Stop whatever is not wanted.
        if want != HotkeyBackend::Evdev {
            if let Some(l) = self.evdev.lock().as_ref() {
                l.disable();
            }
        }
        if want != HotkeyBackend::Portal {
            self.portal.lock().take();
        }

        let mut status = HotkeyStatus::default();
        match want {
            HotkeyBackend::Evdev => {
                status.backend = "evdev".into();
                let mut guard = self.evdev.lock();
                if guard.is_none() {
                    match evdev_backend::EvdevListener::start() {
                        Ok(l) => *guard = Some(l),
                        Err(e) => {
                            status.ok = false;
                            status.message = format!("{e:#}");
                        }
                    }
                }
                if let Some(l) = guard.as_ref() {
                    let errors = l.configure(&binds, tx);
                    status.ok = errors.is_empty();
                    status.message = if errors.is_empty() {
                        "Listening to your keyboards directly.".into()
                    } else {
                        errors.join("; ")
                    };
                }
            }
            HotkeyBackend::Portal => {
                status.backend = "portal".into();
                let key: Vec<String> = binds.iter().map(|b| format!("{}={}", b.slot.id(), b.tokens.join("+"))).collect();
                let mut guard = self.portal.lock();
                let same = guard.as_ref().map(|(k, _)| *k == key).unwrap_or(false);
                if same {
                    status = self.status.lock().clone();
                } else {
                    status.ok = true;
                    status.message = "Registering shortcuts with the desktop…".into();
                    *self.status.lock() = status.clone();
                    let handle = portal_backend::start(binds, tx, self.status.clone());
                    *guard = Some((key, handle));
                    return;
                }
            }
            _ => {
                status.backend = "none".into();
                status.ok = true;
                status.message = "Global hotkeys are off. Use the tray menu or bind `opendictate toggle` in your desktop settings.".into();
            }
        }
        *self.status.lock() = status;
    }

    #[cfg(not(target_os = "linux"))]
    fn apply_native(&self, app: &AppHandle, backend: HotkeyBackend, binds: Vec<Binding>, tx: EventTx) {
        use tauri_plugin_global_shortcut::GlobalShortcutExt;
        let mut status = HotkeyStatus::default();
        if backend == HotkeyBackend::Disabled {
            let _ = app.global_shortcut().unregister_all();
            status.backend = "none".into();
            status.ok = true;
            status.message = "Global hotkeys are off.".into();
        } else {
            status.backend = "native".into();
            match super::native_backend::apply(app, &binds, tx) {
                Ok(()) => {
                    status.ok = true;
                    status.message = "Global shortcuts registered.".into();
                }
                Err(e) => {
                    status.ok = false;
                    status.message = format!("{e:#}");
                }
            }
        }
        *self.status.lock() = status;
    }
}
