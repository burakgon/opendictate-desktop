//! macOS integration.
//!
//! OpenDictate talks to the system frameworks directly here (AppKit, CoreGraphics and the
//! accessibility API) instead of going through cross-platform crates. That is what makes it
//! behave like a native Mac app - a real HUD panel instead of a web view, the user's own keyboard
//! layout for ⌘V, a clipboard that is restored with all of its formats - and it keeps the
//! background footprint small.

pub mod ffi;
pub mod keys;
pub mod overlay;
pub mod pasteboard;
pub mod permissions;
pub mod sound;
pub mod system;

use std::ffi::c_void;

use objc2::MainThreadMarker;

/// Runs `f` on the main thread (asynchronously, unless we are already on it).
///
/// AppKit objects and the Text Input Sources API may only be used from the main thread. Unlike
/// `AppHandle::run_on_main_thread` this needs no handle, so it also works from places such as
/// the sound player.
pub fn on_main<F: FnOnce() + Send + 'static>(f: F) {
    if MainThreadMarker::new().is_some() {
        f();
        return;
    }
    extern "C" fn trampoline(ctx: *mut c_void) {
        // SAFETY: `ctx` is the box leaked below, and GCD calls us exactly once.
        let f = unsafe { Box::from_raw(ctx as *mut Box<dyn FnOnce() + Send>) };
        f();
    }
    let boxed: Box<Box<dyn FnOnce() + Send>> = Box::new(Box::new(f));
    // SAFETY: the main queue lives for the whole process.
    unsafe { ffi::dispatch_async_f(ffi::main_queue(), Box::into_raw(boxed) as *mut c_void, trampoline) }
}
