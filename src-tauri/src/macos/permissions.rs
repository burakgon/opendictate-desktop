//! Privacy permissions: Accessibility (pasting, reading the selection, single-key shortcuts)
//! and the microphone.

use block2::RcBlock;
use objc2::{class, msg_send, rc::Retained, runtime::Bool};
use objc2_foundation::{NSDictionary, NSNumber, NSString};

use super::ffi;

/// True when OpenDictate may post key events and read other apps' accessibility attributes.
/// Never shows a prompt.
pub fn accessibility_trusted() -> bool {
    // SAFETY: plain query without side effects.
    unsafe { ffi::AXIsProcessTrusted() }
}

/// Asks macOS to show its "OpenDictate would like to control this computer" alert, which also
/// adds OpenDictate to the list in System Settings → Privacy & Security → Accessibility.
pub fn request_accessibility() -> bool {
    // SAFETY: `kAXTrustedCheckOptionPrompt` is a CFString constant, toll-free bridged to NSString.
    unsafe {
        let key: &NSString = &*(ffi::kAXTrustedCheckOptionPrompt as *const NSString);
        let yes = NSNumber::new_bool(true);
        let options = NSDictionary::from_slices(&[key], &[&*yes]);
        ffi::AXIsProcessTrustedWithOptions(Retained::as_ptr(&options) as ffi::CFDictionaryRef)
    }
}

#[link(name = "AVFoundation", kind = "framework")]
extern "C" {
    static AVMediaTypeAudio: &'static NSString;
}

/// Microphone access as macOS reports it: `granted`, `denied`, `restricted` or `undetermined`.
pub fn microphone_status() -> &'static str {
    // SAFETY: AVCaptureDevice is linked through AVFoundation; the method returns an NSInteger.
    let status: isize = unsafe { msg_send![class!(AVCaptureDevice), authorizationStatusForMediaType: AVMediaTypeAudio] };
    match status {
        0 => "undetermined",
        1 => "restricted",
        2 => "denied",
        _ => "granted",
    }
}

/// Shows the microphone permission prompt (only the first time; afterwards macOS remembers the
/// answer and the user has to change it in System Settings).
pub fn request_microphone() {
    let handler = RcBlock::new(|granted: Bool| {
        log::info!("microphone access {}", if granted.as_bool() { "granted" } else { "denied" });
    });
    // SAFETY: the block is copied by AVFoundation; the class and media type are valid.
    unsafe {
        let _: () = msg_send![class!(AVCaptureDevice), requestAccessForMediaType: AVMediaTypeAudio, completionHandler: &*handler];
    }
}

/// Opens System Settings at the given privacy pane: `accessibility`, `microphone` or
/// `input_monitoring`.
pub fn open_privacy_pane(pane: &str) -> anyhow::Result<()> {
    let anchor = match pane {
        "accessibility" => "Privacy_Accessibility",
        "microphone" => "Privacy_Microphone",
        "input_monitoring" => "Privacy_ListenEvent",
        other => anyhow::bail!("unknown privacy pane '{other}'"),
    };
    super::system::open_url(&format!("x-apple.systempreferences:com.apple.preference.security?{anchor}"))
}
