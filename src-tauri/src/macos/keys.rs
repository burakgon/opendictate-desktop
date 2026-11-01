//! Synthetic key presses: ⌘V / ⌘C for pasting and reading the selection, and Unicode typing.
//!
//! Events carry their own modifier flags, so a key the user is still holding (say ⌥ from ⌥Space)
//! never turns ⌘V into ⌥⌘V, and they are marked so our own hotkey listener ignores them.

use std::{
    ptr,
    sync::atomic::{AtomicU16, Ordering},
    thread,
    time::Duration,
};

use anyhow::{anyhow, Result};

use super::ffi::{self, CGEventFlags, CGEventRef, CGEventSourceRef, CGKeyCode};

/// Stored in `kCGEventSourceUserData` of every event we post.
pub const EVENT_MARKER: i64 = 0x4F44_4B42; // "ODKB"

const KVK_ANSI_C: CGKeyCode = 8;
const KVK_ANSI_V: CGKeyCode = 9;

/// Key codes that type "v" and "c" in the current keyboard layout (Dvorak, AZERTY, ...).
static KEY_V: AtomicU16 = AtomicU16::new(KVK_ANSI_V);
static KEY_C: AtomicU16 = AtomicU16::new(KVK_ANSI_C);

/// Finds the virtual key that produces `ch` while ⌘ is held, in the given layout.
///
/// # Safety
/// Must run on the main thread (Text Input Sources requirement); `source` must be valid.
unsafe fn find_key(source: ffi::TISInputSourceRef, ch: char) -> Option<CGKeyCode> {
    if source.is_null() {
        return None;
    }
    let data = ffi::TISGetInputSourceProperty(source, ffi::kTISPropertyUnicodeKeyLayoutData);
    if data.is_null() {
        return None;
    }
    let layout = ffi::CFDataGetBytePtr(data) as *const std::ffi::c_void;
    let kbd_type = ffi::LMGetKbdType() as u32;
    // cmdKey (1 << 8) shifted right by 8, as UCKeyTranslate expects: layouts such as
    // "Dvorak - QWERTY ⌘" switch to QWERTY while ⌘ is held.
    const CMD_STATE: u32 = 1;
    for code in 0..128u16 {
        let mut dead = 0u32;
        let mut len = 0usize;
        let mut buf = [0u16; 4];
        let status = ffi::UCKeyTranslate(
            layout,
            code,
            ffi::kUCKeyActionDisplay,
            CMD_STATE,
            kbd_type,
            ffi::kUCKeyTranslateNoDeadKeysMask,
            &mut dead,
            buf.len(),
            &mut len,
            buf.as_mut_ptr(),
        );
        if status == 0 && len == 1 && char::from_u32(buf[0] as u32).map(|c| c.eq_ignore_ascii_case(&ch)).unwrap_or(false) {
            return Some(code);
        }
    }
    None
}

/// Re-reads which keys type "v" and "c". Must run on the main thread; see [`watch_layout`].
pub fn refresh_layout() {
    // SAFETY: called on the main thread; every copied input source is released.
    unsafe {
        let lookup = |ch: char| -> Option<CGKeyCode> {
            for copy in [ffi::TISCopyCurrentKeyboardLayoutInputSource, ffi::TISCopyCurrentASCIICapableKeyboardLayoutInputSource] {
                let source = copy();
                let found = find_key(source, ch);
                if !source.is_null() {
                    ffi::CFRelease(source as ffi::CFTypeRef);
                }
                if found.is_some() {
                    return found;
                }
            }
            None
        };
        let v = lookup('v').unwrap_or(KVK_ANSI_V);
        let c = lookup('c').unwrap_or(KVK_ANSI_C);
        KEY_V.store(v, Ordering::Relaxed);
        KEY_C.store(c, Ordering::Relaxed);
        log::debug!("keyboard layout: v = key {v}, c = key {c}");
    }
}

/// Keeps the cached key codes in sync with the selected keyboard layout. Call once on the main
/// thread at startup.
pub fn watch_layout() {
    use block2::RcBlock;
    use objc2_foundation::{NSDistributedNotificationCenter, NSNotification, NSOperationQueue, NSString};

    refresh_layout();
    // SAFETY: the notification name is a CFString constant (toll-free bridged); the observer
    // block runs on the main queue and lives as long as the process (it is never removed).
    unsafe {
        let name: &NSString = &*(ffi::kTISNotifySelectedKeyboardInputSourceChanged as *const NSString);
        let block = RcBlock::new(|_: ptr::NonNull<NSNotification>| refresh_layout());
        let center = NSDistributedNotificationCenter::defaultCenter();
        let observer = center.addObserverForName_object_queue_usingBlock(Some(name), None, Some(&NSOperationQueue::mainQueue()), &block);
        std::mem::forget(observer);
    }
}

struct Source(CGEventSourceRef);

impl Source {
    fn new() -> Self {
        // SAFETY: creating an event source has no preconditions; NULL is allowed by the API.
        unsafe {
            let src = ffi::CGEventSourceCreate(ffi::kCGEventSourceStateCombinedSessionState);
            if !src.is_null() {
                // Don't swallow the user's own key presses right after ours.
                ffi::CGEventSourceSetLocalEventsFilterDuringSuppressionState(
                    src,
                    ffi::kCGEventFilterMaskPermitAllEvents,
                    ffi::kCGEventSuppressionStateSuppressionInterval,
                );
                ffi::CGEventSourceSetLocalEventsSuppressionInterval(src, 0.0);
            }
            Source(src)
        }
    }
}

impl Drop for Source {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: we own the source.
            unsafe { ffi::CFRelease(self.0 as ffi::CFTypeRef) }
        }
    }
}

struct Event(CGEventRef);

impl Event {
    fn key(source: &Source, code: CGKeyCode, down: bool, flags: CGEventFlags) -> Result<Self> {
        // SAFETY: a NULL source is accepted; the event is released on drop.
        unsafe {
            let ev = ffi::CGEventCreateKeyboardEvent(source.0, code, down);
            if ev.is_null() {
                return Err(anyhow!("could not create a key event"));
            }
            ffi::CGEventSetFlags(ev, flags | ffi::kCGEventFlagMaskNonCoalesced);
            ffi::CGEventSetIntegerValueField(ev, ffi::kCGEventSourceUserData, EVENT_MARKER);
            Ok(Event(ev))
        }
    }

    fn post(&self) {
        // SAFETY: posting a valid event.
        unsafe { ffi::CGEventPost(ffi::kCGSessionEventTap, self.0) }
    }
}

impl Drop for Event {
    fn drop(&mut self) {
        // SAFETY: we own the event.
        unsafe { ffi::CFRelease(self.0 as ffi::CFTypeRef) }
    }
}

fn require_trust() -> Result<()> {
    if super::permissions::accessibility_trusted() {
        Ok(())
    } else {
        Err(anyhow!("OpenDictate needs Accessibility access to type into other apps (System Settings → Privacy & Security → Accessibility)"))
    }
}

/// Presses `code` together with ⌘ (and ⇧ if `shift`).
fn command_chord(code: CGKeyCode, shift: bool) -> Result<()> {
    require_trust()?;
    let mut flags = ffi::kCGEventFlagMaskCommand | ffi::NX_DEVICELCMDKEYMASK;
    if shift {
        flags |= ffi::kCGEventFlagMaskShift | ffi::NX_DEVICELSHIFTKEYMASK;
    }
    let source = Source::new();
    let down = Event::key(&source, code, true, flags)?;
    // The key-up carries the modifiers that are really held. Otherwise the session's modifier
    // state (what other apps read from `NSEvent.modifierFlags`) would keep ⌘ "pressed" after us.
    let up = Event::key(&source, code, false, held_modifiers())?;
    down.post();
    thread::sleep(Duration::from_millis(8));
    up.post();
    Ok(())
}

/// Modifier flags of the keys physically held right now.
fn held_modifiers() -> CGEventFlags {
    const MODIFIER_BITS: CGEventFlags = ffi::kCGEventFlagMaskShift
        | ffi::kCGEventFlagMaskControl
        | ffi::kCGEventFlagMaskAlternate
        | ffi::kCGEventFlagMaskCommand
        | ffi::kCGEventFlagMaskSecondaryFn
        | 0x0000_FFFF; // device-dependent bits (left/right)
    // SAFETY: plain query.
    unsafe { ffi::CGEventSourceFlagsState(ffi::kCGEventSourceStateHIDSystemState) & MODIFIER_BITS }
}

/// ⌘V (or ⇧⌘V).
pub fn paste(shift: bool) -> Result<()> {
    command_chord(KEY_V.load(Ordering::Relaxed), shift)
}

/// ⌘C.
pub fn copy() -> Result<()> {
    command_chord(KEY_C.load(Ordering::Relaxed), false)
}

/// Types `text` as Unicode key events, independent of the keyboard layout.
pub fn type_text(text: &str) -> Result<()> {
    require_trust()?;
    let source = Source::new();
    // Up to 20 UTF-16 units per event is what apps reliably accept; never split a surrogate pair.
    let mut chunk: Vec<u16> = Vec::with_capacity(20);
    let flush = |chunk: &mut Vec<u16>| -> Result<()> {
        if chunk.is_empty() {
            return Ok(());
        }
        for down in [true, false] {
            let ev = Event::key(&source, 0, down, if down { 0 } else { held_modifiers() })?;
            // SAFETY: valid event and buffer.
            unsafe { ffi::CGEventKeyboardSetUnicodeString(ev.0, chunk.len(), chunk.as_ptr()) };
            ev.post();
        }
        chunk.clear();
        thread::sleep(Duration::from_millis(3));
        Ok(())
    };
    let mut units = [0u16; 2];
    for ch in text.chars() {
        let encoded = ch.encode_utf16(&mut units);
        if chunk.len() + encoded.len() > 20 {
            flush(&mut chunk)?;
        }
        chunk.extend_from_slice(encoded);
    }
    flush(&mut chunk)
}
