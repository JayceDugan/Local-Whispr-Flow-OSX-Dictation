//! Paste injection: standard dictation-tool approach — write the transcript
//! to the general pasteboard, then synthesize ⌘V via CoreGraphics. The
//! transcript intentionally stays on the clipboard afterwards (product
//! requirement), so there is no save/restore dance.

use std::{thread, time::Duration};

use arboard::Clipboard;
use core_foundation::base::TCFType;
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFDictionary;
use core_foundation::string::CFString;
use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation, CGKeyCode};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

/// Virtual keycode for 'V' on an ANSI layout.
const KEY_V: CGKeyCode = 9;

pub fn inject_text(text: &str) -> Result<(), crate::error::ClientError> {
    set_clipboard(text)?;
    // Let the pasteboard write settle before synthesizing ⌘V, otherwise a
    // slow target app can read the previous clipboard contents.
    thread::sleep(Duration::from_millis(60));
    send_cmd_v()
}

fn set_clipboard(text: &str) -> Result<(), crate::error::ClientError> {
    Clipboard::new()
        .and_then(|mut c| c.set_text(text.to_string()))
        .map_err(|e| crate::error::ClientError::audio(format!("clipboard write failed: {e}")))
}

fn send_cmd_v() -> Result<(), crate::error::ClientError> {
    let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState)
        .map_err(|_| crate::error::ClientError::audio("could not create event source"))?;

    let down = CGEvent::new_keyboard_event(source.clone(), KEY_V, true)
        .map_err(|_| crate::error::ClientError::audio("failed to create keydown event"))?;
    down.set_flags(CGEventFlags::CGEventFlagCommand);
    down.post(CGEventTapLocation::HID);

    let up = CGEvent::new_keyboard_event(source, KEY_V, false)
        .map_err(|_| crate::error::ClientError::audio("failed to create keyup event"))?;
    up.set_flags(CGEventFlags::CGEventFlagCommand);
    up.post(CGEventTapLocation::HID);

    Ok(())
}

/// Accessibility trust gate for synthesized ⌘V. Without it, CGEvent posting
/// silently does nothing — hence the explicit preflight for the UI.
pub fn is_trusted_for_accessibility(prompt: bool) -> bool {
    if prompt {
        let key = CFString::new("AXTrustedCheckOptionPrompt");
        let value = CFBoolean::from(true);
        let dict = CFDictionary::from_CFType_pairs(&[(key, value)]);
        unsafe { AXIsProcessTrustedWithOptions(dict.as_CFTypeRef() as *const std::ffi::c_void) }
    } else {
        unsafe { AXIsProcessTrusted() }
    }
}

pub fn open_accessibility_pane() {
    // Deep-link straight at the Accessibility settings list.
    let _ = std::process::Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
        .spawn();
}

extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: *const std::ffi::c_void) -> bool;
}

