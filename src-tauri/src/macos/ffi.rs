//! The few C APIs we call directly (CoreGraphics events, CoreFoundation run loops,
//! the accessibility API, Text Input Sources and libdispatch).

#![allow(non_upper_case_globals, non_camel_case_types, dead_code)]

use std::ffi::c_void;

pub type CFTypeRef = *const c_void;
pub type CFStringRef = *const c_void;
pub type CFDictionaryRef = *const c_void;
pub type CFDataRef = *const c_void;
pub type CFMachPortRef = *mut c_void;
pub type CFRunLoopRef = *mut c_void;
pub type CFRunLoopSourceRef = *mut c_void;
pub type CFTypeID = usize;

pub type CGEventRef = *mut c_void;
pub type CGEventSourceRef = *mut c_void;
pub type CGEventTapProxy = *mut c_void;
pub type CGEventType = u32;
pub type CGEventMask = u64;
pub type CGEventFlags = u64;
pub type CGKeyCode = u16;
pub type CGEventTapCallBack =
    extern "C" fn(proxy: CGEventTapProxy, kind: CGEventType, event: CGEventRef, user_info: *mut c_void) -> CGEventRef;

pub type AXUIElementRef = *const c_void;
pub type AXError = i32;
pub const kAXErrorSuccess: AXError = 0;

// Event taps and posting.
pub const kCGHIDEventTap: u32 = 0;
pub const kCGSessionEventTap: u32 = 1;
pub const kCGHeadInsertEventTap: u32 = 0;
pub const kCGEventTapOptionDefault: u32 = 0;
pub const kCGEventTapOptionListenOnly: u32 = 1;
pub const kCGEventSourceStateCombinedSessionState: i32 = 0;
pub const kCGEventSourceStateHIDSystemState: i32 = 1;
pub const kCGEventSourceStatePrivate: i32 = -1;

pub const kCGEventLeftMouseDown: CGEventType = 1;
pub const kCGEventRightMouseDown: CGEventType = 3;
pub const kCGEventKeyDown: CGEventType = 10;
pub const kCGEventKeyUp: CGEventType = 11;
pub const kCGEventFlagsChanged: CGEventType = 12;
pub const kCGEventOtherMouseDown: CGEventType = 25;
pub const kCGEventOtherMouseUp: CGEventType = 26;
pub const kCGEventTapDisabledByTimeout: CGEventType = 0xFFFF_FFFE;
pub const kCGEventTapDisabledByUserInput: CGEventType = 0xFFFF_FFFF;

pub const kCGMouseEventButtonNumber: u32 = 3;
pub const kCGKeyboardEventAutorepeat: u32 = 8;
pub const kCGKeyboardEventKeycode: u32 = 9;
pub const kCGEventSourceUserData: u32 = 42;

pub const kCGEventFlagMaskShift: CGEventFlags = 0x0002_0000;
pub const kCGEventFlagMaskControl: CGEventFlags = 0x0004_0000;
pub const kCGEventFlagMaskAlternate: CGEventFlags = 0x0008_0000;
pub const kCGEventFlagMaskCommand: CGEventFlags = 0x0010_0000;
pub const kCGEventFlagMaskSecondaryFn: CGEventFlags = 0x0080_0000;
pub const kCGEventFlagMaskNonCoalesced: CGEventFlags = 0x0000_0100;
// Device-dependent bits that tell the left and right modifier keys apart (IOLLEvent.h).
pub const NX_DEVICELCTLKEYMASK: CGEventFlags = 0x0000_0001;
pub const NX_DEVICELSHIFTKEYMASK: CGEventFlags = 0x0000_0002;
pub const NX_DEVICERSHIFTKEYMASK: CGEventFlags = 0x0000_0004;
pub const NX_DEVICELCMDKEYMASK: CGEventFlags = 0x0000_0008;
pub const NX_DEVICERCMDKEYMASK: CGEventFlags = 0x0000_0010;
pub const NX_DEVICELALTKEYMASK: CGEventFlags = 0x0000_0020;
pub const NX_DEVICERALTKEYMASK: CGEventFlags = 0x0000_0040;
pub const NX_DEVICERCTLKEYMASK: CGEventFlags = 0x0000_2000;

pub const kCGEventFilterMaskPermitAllEvents: u32 = 0b111;
pub const kCGEventSuppressionStateSuppressionInterval: u32 = 0;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    pub fn CGEventSourceCreate(state: i32) -> CGEventSourceRef;
    pub fn CGEventSourceSetLocalEventsFilterDuringSuppressionState(source: CGEventSourceRef, filter: u32, state: u32);
    pub fn CGEventSourceSetLocalEventsSuppressionInterval(source: CGEventSourceRef, seconds: f64);
    pub fn CGEventCreateKeyboardEvent(source: CGEventSourceRef, keycode: CGKeyCode, key_down: bool) -> CGEventRef;
    pub fn CGEventKeyboardSetUnicodeString(event: CGEventRef, length: usize, string: *const u16);
    pub fn CGEventSetFlags(event: CGEventRef, flags: CGEventFlags);
    pub fn CGEventGetFlags(event: CGEventRef) -> CGEventFlags;
    pub fn CGEventSetIntegerValueField(event: CGEventRef, field: u32, value: i64);
    pub fn CGEventGetIntegerValueField(event: CGEventRef, field: u32) -> i64;
    pub fn CGEventPost(tap: u32, event: CGEventRef);
    pub fn CGEventTapCreate(
        tap: u32,
        place: u32,
        options: u32,
        events_of_interest: CGEventMask,
        callback: CGEventTapCallBack,
        user_info: *mut c_void,
    ) -> CFMachPortRef;
    pub fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
    pub fn CGPreflightListenEventAccess() -> bool;
    pub fn CGEventSourceFlagsState(state: i32) -> CGEventFlags;

    pub fn AXIsProcessTrusted() -> bool;
    pub fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> bool;
    pub static kAXTrustedCheckOptionPrompt: CFStringRef;
    pub fn AXUIElementCreateSystemWide() -> AXUIElementRef;
    pub fn AXUIElementCopyAttributeValue(element: AXUIElementRef, attribute: CFStringRef, value: *mut CFTypeRef) -> AXError;
    pub fn AXUIElementSetMessagingTimeout(element: AXUIElementRef, timeout_seconds: f32) -> AXError;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    pub fn CFRelease(cf: CFTypeRef);
    pub fn CFGetTypeID(cf: CFTypeRef) -> CFTypeID;
    pub fn CFStringGetTypeID() -> CFTypeID;
    pub fn CFDataGetBytePtr(data: CFDataRef) -> *const u8;
    pub fn CFMachPortCreateRunLoopSource(allocator: CFTypeRef, port: CFMachPortRef, order: isize) -> CFRunLoopSourceRef;
    pub fn CFMachPortInvalidate(port: CFMachPortRef);
    pub fn CFRunLoopGetCurrent() -> CFRunLoopRef;
    pub fn CFRunLoopAddSource(rl: CFRunLoopRef, source: CFRunLoopSourceRef, mode: CFStringRef);
    pub fn CFRunLoopRun();
    pub fn CFRunLoopStop(rl: CFRunLoopRef);
    pub fn CFRunLoopWakeUp(rl: CFRunLoopRef);
    pub static kCFRunLoopCommonModes: CFStringRef;
}

// Keyboard layouts (HIToolbox). Main thread only.
pub type TISInputSourceRef = *mut c_void;
pub const kUCKeyActionDisplay: u16 = 3;
pub const kUCKeyTranslateNoDeadKeysMask: u32 = 1;

#[link(name = "Carbon", kind = "framework")]
extern "C" {
    pub fn TISCopyCurrentKeyboardLayoutInputSource() -> TISInputSourceRef;
    pub fn TISCopyCurrentASCIICapableKeyboardLayoutInputSource() -> TISInputSourceRef;
    pub fn TISGetInputSourceProperty(source: TISInputSourceRef, key: CFStringRef) -> *const c_void;
    pub static kTISPropertyUnicodeKeyLayoutData: CFStringRef;
    pub static kTISNotifySelectedKeyboardInputSourceChanged: CFStringRef;
    pub fn LMGetKbdType() -> u8;
    pub fn UCKeyTranslate(
        key_layout: *const c_void,
        virtual_key_code: u16,
        key_action: u16,
        modifier_key_state: u32,
        keyboard_type: u32,
        key_translate_options: u32,
        dead_key_state: *mut u32,
        max_string_length: usize,
        actual_string_length: *mut usize,
        unicode_string: *mut u16,
    ) -> i32;
}

// libdispatch (part of libSystem).
#[repr(C)]
pub struct dispatch_queue_s {
    _private: [u8; 0],
}

extern "C" {
    static _dispatch_main_q: dispatch_queue_s;
    pub fn dispatch_async_f(queue: *const dispatch_queue_s, context: *mut c_void, work: extern "C" fn(*mut c_void));
    pub fn dispatch_sync_f(queue: *const dispatch_queue_s, context: *mut c_void, work: extern "C" fn(*mut c_void));
}

/// `dispatch_get_main_queue()`, which is a macro in C.
pub fn main_queue() -> *const dispatch_queue_s {
    // SAFETY: taking the address of a static that libdispatch exports for exactly this purpose.
    unsafe { &_dispatch_main_q as *const dispatch_queue_s }
}
