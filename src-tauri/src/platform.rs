//! Environment detection and (on Linux) a one-click permission fix.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Environment {
    pub os: &'static str,
    pub session_type: String,
    pub desktop: String,
    /// Linux: can read /dev/input (needed for the evdev hotkey backend).
    pub input_access: bool,
    /// Linux: can write /dev/uinput (needed to paste into other apps).
    pub uinput_access: bool,
    /// Linux: the user is in the `input` group but this session predates it (re-login needed).
    pub relogin_required: bool,
    pub layer_shell: bool,
    /// The floating overlay can be used on this desktop.
    pub overlay_available: bool,
}

pub fn environment() -> Environment {
    #[cfg(target_os = "linux")]
    {
        use std::fs::OpenOptions;
        let session_type = std::env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| {
            if std::env::var_os("WAYLAND_DISPLAY").is_some() {
                "wayland".into()
            } else {
                "x11".into()
            }
        });
        let input_access = crate::hotkeys::evdev_backend::can_read_input_devices();
        let uinput_access = OpenOptions::new().write(true).open("/dev/uinput").is_ok();
        Environment {
            os: "linux",
            session_type,
            desktop: std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default(),
            input_access,
            uinput_access,
            relogin_required: !input_access && user_listed_in_group("input"),
            layer_shell: crate::overlay::uses_layer_shell(),
            overlay_available: crate::overlay::available(),
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        Environment {
            os: if cfg!(target_os = "macos") { "macos" } else { "windows" },
            session_type: String::new(),
            desktop: String::new(),
            input_access: true,
            uinput_access: true,
            relogin_required: false,
            layer_shell: false,
            overlay_available: true,
        }
    }
}

#[cfg(target_os = "linux")]
fn user_listed_in_group(group: &str) -> bool {
    let user = std::env::var("USER").unwrap_or_default();
    if user.is_empty() {
        return false;
    }
    std::fs::read_to_string("/etc/group")
        .map(|g| {
            g.lines().any(|line| {
                let mut parts = line.split(':');
                parts.next() == Some(group) && parts.nth(2).map(|m| m.split(',').any(|u| u == user)).unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

#[cfg(target_os = "linux")]
const FIX_SCRIPT: &str = r#"set -e
cat > /etc/udev/rules.d/70-opendictate.rules <<'RULES'
# OpenDictate: give the user at the active seat access to input devices (global hotkeys)
# and to uinput (pasting through a virtual keyboard). Must sort before 73-seat-late.rules.
KERNEL=="uinput", SUBSYSTEM=="misc", TAG+="uaccess", OPTIONS+="static_node=uinput"
KERNEL=="event*", SUBSYSTEM=="input", TAG+="uaccess"
RULES
echo uinput > /etc/modules-load.d/opendictate-uinput.conf
modprobe uinput || true
udevadm control --reload-rules || true
udevadm trigger --subsystem-match=input --subsystem-match=misc --action=change || true
udevadm settle || true
"#;

/// Installs a udev rule that gives the logged-in user (via logind's `uaccess`) access to
/// keyboards (global hotkeys) and /dev/uinput (pasting). Takes effect immediately, no re-login.
/// Runs through pkexec so the desktop shows its normal password prompt.
#[cfg(target_os = "linux")]
pub fn fix_permissions(_include_input_group: bool) -> anyhow::Result<String> {
    use std::process::Command;
    let status = Command::new("pkexec").args(["sh", "-c", FIX_SCRIPT]).status()?;
    if !status.success() {
        anyhow::bail!("the permission helper was cancelled or failed ({status})");
    }
    Ok("Permissions granted. Shortcuts and pasting are ready.".into())
}

#[cfg(not(target_os = "linux"))]
pub fn fix_permissions(_include_input_group: bool) -> anyhow::Result<String> {
    Ok("Nothing to fix on this platform.".into())
}

/// Desktop portals identify host apps by their `.desktop` file. Make sure one named after our
/// app id exists (packages ship `opendictate.desktop`; AppImages and dev builds ship none).
#[cfg(target_os = "linux")]
pub fn ensure_desktop_entry() {
    use std::{fs, path::PathBuf};

    const APP_ID: &str = crate::hotkeys::portal_backend::APP_ID;
    let file_name = format!("{APP_ID}.desktop");
    let home = match std::env::var_os("HOME") {
        Some(h) => PathBuf::from(h),
        None => return,
    };
    let data_home = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".local/share"));
    let data_dirs: Vec<PathBuf> = std::env::var("XDG_DATA_DIRS")
        .unwrap_or_else(|_| "/usr/local/share:/usr/share".into())
        .split(':')
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .collect();

    let exe = std::env::var_os("APPIMAGE")
        .map(PathBuf::from)
        .or_else(|| std::env::current_exe().ok())
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "opendictate".into());

    let target = data_home.join("applications").join(&file_name);
    // A system-wide entry with our id wins. Remove a stale one we generated earlier (e.g. from
    // an AppImage or a dev build) so it doesn't shadow the packaged entry.
    if data_dirs.iter().any(|d| d.join("applications").join(&file_name).exists()) {
        if fs::read_to_string(&target).map(|t| t.contains("X-OpenDictate-Generated=true")).unwrap_or(false) {
            let _ = fs::remove_file(&target);
        }
        return;
    }
    let packaged = data_dirs
        .iter()
        .any(|d| d.join("applications/opendictate.desktop").exists() || d.join("applications/OpenDictate.desktop").exists());

    if let Ok(existing) = fs::read_to_string(&target) {
        let ours = existing.contains("X-OpenDictate-Generated=true");
        let same_exe = existing.contains(&format!("Exec=\"{exe}\""));
        if !ours || same_exe {
            return;
        }
    }

    let icon_dir = data_home.join("icons/hicolor/128x128/apps");
    let icon_path = icon_dir.join(format!("{APP_ID}.png"));
    if !icon_path.exists() {
        let _ = fs::create_dir_all(&icon_dir);
        let _ = fs::write(&icon_path, include_bytes!("../icons/128x128.png"));
    }
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName=OpenDictate\nGenericName=Voice Dictation\nComment=Voice dictation and voice commands for your desktop\nExec=\"{exe}\" %U\nIcon={APP_ID}\nTerminal=false\nCategories=Utility;Accessibility;\nKeywords=dictation;speech;voice;transcription;whisper;\nStartupWMClass=opendictate\nNoDisplay={}\nX-OpenDictate-Generated=true\n",
        if packaged { "true" } else { "false" }
    );
    if let Some(dir) = target.parent() {
        let _ = fs::create_dir_all(dir);
    }
    match fs::write(&target, entry) {
        Ok(()) => log::info!("registered desktop entry {}", target.display()),
        Err(e) => log::warn!("could not write {}: {e}", target.display()),
    }
}

#[cfg(not(target_os = "linux"))]
pub fn ensure_desktop_entry() {}
