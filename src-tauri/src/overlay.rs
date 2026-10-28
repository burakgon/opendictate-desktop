//! The floating status pill shown while listening / thinking.
//!
//! On Wayland compositors that support `wlr-layer-shell` (KDE Plasma, Hyprland, Sway, ...)
//! the window becomes a layer surface: it is anchored to the screen edge, never takes keyboard
//! focus and lets clicks pass through. Elsewhere it is a borderless, non-focusable,
//! always-on-top window placed near the bottom of the screen.

use tauri::{AppHandle, LogicalSize, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::settings::OverlayPosition;

pub const LABEL: &str = "overlay";
const WIDTH: f64 = 480.0;
const HEIGHT: f64 = 132.0;
const MARGIN: i32 = 28;

pub fn create(app: &AppHandle, position: OverlayPosition) -> tauri::Result<WebviewWindow> {
    let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("overlay.html".into()))
        .title("OpenDictate")
        .inner_size(WIDTH, HEIGHT)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .focused(false)
        .focusable(false)
        .shadow(false)
        .visible(false)
        .visible_on_all_workspaces(true)
        .build()?;

    #[cfg(target_os = "linux")]
    {
        // Click-through is applied after the window is shown (see `show`): tao panics when
        // asked to change the input region of a window that isn't realized yet.
        let layered = linux::init_layer_shell(&window, position);
        LAYER_SHELL.store(layered, std::sync::atomic::Ordering::Relaxed);
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = window.set_ignore_cursor_events(true);
    }
    Ok(window)
}

#[cfg(target_os = "linux")]
static LAYER_SHELL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether the overlay can be shown without risking focus theft. On Wayland compositors without
/// layer-shell (GNOME) a regular window can't stay on top and might take keyboard focus, which
/// would swallow the pasted text - so the overlay stays off there (sounds + tray still work).
pub fn available() -> bool {
    #[cfg(target_os = "linux")]
    {
        let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some()
            && std::env::var("GDK_BACKEND").map(|b| !b.starts_with("x11")).unwrap_or(true);
        uses_layer_shell() || !wayland || std::env::var_os("OPENDICTATE_FORCE_OVERLAY").is_some()
    }
    #[cfg(not(target_os = "linux"))]
    {
        true
    }
}

pub fn uses_layer_shell() -> bool {
    #[cfg(target_os = "linux")]
    {
        LAYER_SHELL.load(std::sync::atomic::Ordering::Relaxed)
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

fn place(window: &WebviewWindow, position: OverlayPosition) {
    if uses_layer_shell() {
        #[cfg(target_os = "linux")]
        linux::set_edge(window, position);
        return;
    }
    let monitor = window.current_monitor().ok().flatten().or_else(|| window.primary_monitor().ok().flatten());
    if let Some(m) = monitor {
        let scale = m.scale_factor();
        let area = m.work_area();
        let w = (WIDTH * scale) as i32;
        let h = (HEIGHT * scale) as i32;
        let x = area.position.x + (area.size.width as i32 - w) / 2;
        let y = match position {
            OverlayPosition::Bottom => area.position.y + area.size.height as i32 - h - (MARGIN as f64 * scale) as i32,
            OverlayPosition::Top => area.position.y + (MARGIN as f64 * scale) as i32,
        };
        let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
    }
}

pub fn show(app: &AppHandle, position: OverlayPosition) {
    if !available() {
        return;
    }
    let app2 = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(w) = app2.get_webview_window(LABEL) {
            if !w.is_visible().unwrap_or(false) {
                log::debug!("overlay: show");
                place(&w, position);
                let _ = w.set_size(LogicalSize::new(WIDTH, HEIGHT));
                let _ = w.show();
                #[cfg(target_os = "linux")]
                linux::make_click_through(&w);
            }
        }
    });
}

pub fn hide(app: &AppHandle) {
    let app2 = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(w) = app2.get_webview_window(LABEL) {
            let _ = w.hide();
        }
    });
}

#[cfg(target_os = "linux")]
mod linux {
    use std::{
        ffi::{c_int, c_void, CString},
        sync::OnceLock,
    };

    use gtk::prelude::*;
    use libloading::{Library, Symbol};
    use tauri::WebviewWindow;

    use super::{OverlayPosition, HEIGHT, MARGIN, WIDTH};

    const LAYER_OVERLAY: c_int = 3;
    const EDGE_TOP: c_int = 2;
    const EDGE_BOTTOM: c_int = 3;
    const KEYBOARD_NONE: c_int = 0;

    struct LayerShell {
        lib: Library,
    }

    fn lib() -> Option<&'static LayerShell> {
        static LIB: OnceLock<Option<LayerShell>> = OnceLock::new();
        LIB.get_or_init(|| {
            for name in ["libgtk-layer-shell.so.0", "libgtk-layer-shell.so"] {
                if let Ok(lib) = unsafe { Library::new(name) } {
                    return Some(LayerShell { lib });
                }
            }
            log::info!("gtk-layer-shell not installed; the overlay uses a regular window");
            None
        })
        .as_ref()
    }

    impl LayerShell {
        unsafe fn call0<R>(&self, name: &[u8]) -> Option<R> {
            let f: Symbol<unsafe extern "C" fn() -> R> = self.lib.get(name).ok()?;
            Some(f())
        }
        unsafe fn call1(&self, name: &[u8], w: *mut c_void) {
            if let Ok(f) = self.lib.get::<unsafe extern "C" fn(*mut c_void)>(name) {
                f(w)
            }
        }
        unsafe fn call2(&self, name: &[u8], w: *mut c_void, a: c_int) {
            if let Ok(f) = self.lib.get::<unsafe extern "C" fn(*mut c_void, c_int)>(name) {
                f(w, a)
            }
        }
        unsafe fn call3(&self, name: &[u8], w: *mut c_void, a: c_int, b: c_int) {
            if let Ok(f) = self.lib.get::<unsafe extern "C" fn(*mut c_void, c_int, c_int)>(name) {
                f(w, a, b)
            }
        }
    }

    fn is_wayland() -> bool {
        std::env::var_os("WAYLAND_DISPLAY").is_some()
            && std::env::var("GDK_BACKEND").map(|b| !b.starts_with("x11")).unwrap_or(true)
    }

    pub fn init_layer_shell(window: &WebviewWindow, position: OverlayPosition) -> bool {
        if !is_wayland() || std::env::var_os("OPENDICTATE_NO_LAYER_SHELL").is_some() {
            return false;
        }
        let Some(ls) = lib() else { return false };
        let Ok(gtk_window) = window.gtk_window() else { return false };
        unsafe {
            if ls.call0::<c_int>(b"gtk_layer_is_supported\0").unwrap_or(0) == 0 {
                log::info!("compositor has no layer-shell support; using a regular overlay window");
                return false;
            }
            log::debug!("overlay: realized before layer-shell init: {}", gtk_window.is_realized());
            if gtk_window.is_realized() {
                gtk_window.unrealize();
            }
            // Layer surfaces take their size from the widget's size request; a WebKit view has
            // no natural size, so without this the surface collapses and nothing is drawn.
            gtk_window.set_size_request(WIDTH as i32, HEIGHT as i32);
            if let Some(child) = gtk_window.child() {
                child.set_size_request(WIDTH as i32, HEIGHT as i32);
            }
            let ptr = gtk_window.as_ptr() as *mut c_void;
            ls.call1(b"gtk_layer_init_for_window\0", ptr);
            ls.call2(b"gtk_layer_set_layer\0", ptr, LAYER_OVERLAY);
            ls.call2(b"gtk_layer_set_keyboard_mode\0", ptr, KEYBOARD_NONE);
            ls.call2(b"gtk_layer_set_exclusive_zone\0", ptr, -1);
            if let Ok(f) = ls.lib.get::<unsafe extern "C" fn(*mut c_void, *const std::ffi::c_char)>(b"gtk_layer_set_namespace\0") {
                let ns = CString::new("opendictate").unwrap();
                f(ptr, ns.as_ptr());
            }
        }
        set_edge(window, position);
        log::info!("overlay: using wlr-layer-shell");
        true
    }

    pub fn set_edge(window: &WebviewWindow, position: OverlayPosition) {
        let (Some(ls), Ok(gtk_window)) = (lib(), window.gtk_window()) else { return };
        let ptr = gtk_window.as_ptr() as *mut c_void;
        let (edge, other) = match position {
            OverlayPosition::Bottom => (EDGE_BOTTOM, EDGE_TOP),
            OverlayPosition::Top => (EDGE_TOP, EDGE_BOTTOM),
        };
        unsafe {
            ls.call3(b"gtk_layer_set_anchor\0", ptr, other, 0);
            ls.call3(b"gtk_layer_set_anchor\0", ptr, edge, 1);
            ls.call3(b"gtk_layer_set_margin\0", ptr, edge, MARGIN - 12);
        }
    }

    /// Lets pointer events fall through the transparent overlay.
    pub fn make_click_through(window: &WebviewWindow) {
        if let Ok(gtk_window) = window.gtk_window() {
            let region = gtk::cairo::Region::create();
            gtk_window.input_shape_combine_region(Some(&region));
        }
    }
}
