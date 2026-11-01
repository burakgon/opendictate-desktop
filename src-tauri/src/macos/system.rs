//! Small system queries: the focused app, the selected text (through the accessibility API)
//! and opening URLs.

use std::{cell::RefCell, ptr};

use anyhow::{anyhow, Result};
use objc2::{rc::Retained, runtime::ProtocolObject};
use objc2_app_kit::NSWorkspace;
use objc2_foundation::{NSActivityOptions, NSObjectProtocol, NSProcessInfo, NSString, NSURL};

use super::ffi::{self, CFTypeRef};

pub fn open_url(url: &str) -> Result<()> {
    let url = NSURL::URLWithString(&NSString::from_str(url)).ok_or_else(|| anyhow!("invalid URL {url}"))?;
    if NSWorkspace::sharedWorkspace().openURL(&url) {
        Ok(())
    } else {
        Err(anyhow!("could not open {url:?}"))
    }
}

/// Name of the application that has keyboard focus, e.g. "Safari".
pub fn frontmost_app() -> Option<String> {
    let app = NSWorkspace::sharedWorkspace().frontmostApplication()?;
    app.localizedName().map(|n| n.to_string()).or_else(|| app.bundleIdentifier().map(|b| b.to_string()))
}

/// An owned CoreFoundation reference, released on drop.
struct Cf(CFTypeRef);

impl Drop for Cf {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: we own one reference (Create/Copy rule).
            unsafe { ffi::CFRelease(self.0) }
        }
    }
}

fn copy_attribute(element: CFTypeRef, name: &str) -> Option<Cf> {
    let attr = NSString::from_str(name);
    let mut value: CFTypeRef = ptr::null();
    // SAFETY: `element` is a live AXUIElement, `attr` a CFString (toll-free bridged NSString).
    let err = unsafe { ffi::AXUIElementCopyAttributeValue(element, Retained::as_ptr(&attr) as ffi::CFStringRef, &mut value) };
    (err == ffi::kAXErrorSuccess && !value.is_null()).then_some(Cf(value))
}

/// The text selected in the focused application, read through the accessibility API - without
/// touching the clipboard or pressing ⌘C. `None` when the app doesn't expose it (many Electron
/// and web apps); the caller then falls back to copying.
pub fn selected_text() -> Option<String> {
    if !super::permissions::accessibility_trusted() {
        return None;
    }
    // SAFETY: standard AX calls on objects we own; every Copy result is released by `Cf`.
    unsafe {
        let system = Cf(ffi::AXUIElementCreateSystemWide());
        if system.0.is_null() {
            return None;
        }
        // Never hang on an unresponsive app.
        ffi::AXUIElementSetMessagingTimeout(system.0, 0.25);
        let focused = copy_attribute(system.0, "AXFocusedUIElement")?;
        let value = copy_attribute(focused.0, "AXSelectedText")?;
        if ffi::CFGetTypeID(value.0) != ffi::CFStringGetTypeID() {
            return None;
        }
        let text = (*(value.0 as *const NSString)).to_string();
        (!text.trim().is_empty()).then_some(text)
    }
}

thread_local! {
    /// Main thread only.
    static ACTIVITY: RefCell<Option<Retained<ProtocolObject<dyn NSObjectProtocol>>>> = const { RefCell::new(None) };
}

/// While OpenDictate is listening or working, keep macOS from App-Napping it: the hold and
/// feedback timers must fire on time even before the overlay is on screen.
pub fn set_busy(busy: bool) {
    super::on_main(move || {
        ACTIVITY.with(|a| {
            let mut a = a.borrow_mut();
            let info = NSProcessInfo::processInfo();
            if busy && a.is_none() {
                let options = NSActivityOptions::UserInitiatedAllowingIdleSystemSleep | NSActivityOptions::LatencyCritical;
                *a = Some(info.beginActivityWithOptions_reason(options, &NSString::from_str("Dictation in progress")));
            } else if !busy {
                if let Some(token) = a.take() {
                    // SAFETY: the token came from `beginActivityWithOptions_reason`.
                    unsafe { info.endActivity(&token) };
                }
            }
        })
    });
}

