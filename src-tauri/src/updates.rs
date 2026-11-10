//! Updates. Once a day (and when asked) OpenDictate looks at the latest GitHub release.
//!
//! Copies that can replace themselves - macOS, Windows and the Linux AppImage - download the
//! update, verify its signature against the public key in `tauri.conf.json`, install it and
//! restart, when the user says so. Other packages (deb, rpm, AUR) link to the release page.

use std::time::Duration;

use anyhow::{anyhow, Result};
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::UpdaterExt;

const RELEASES_URL: &str = "https://github.com/burakgon/opendictate-desktop/releases";
const LATEST_API_URL: &str = "https://api.github.com/repos/burakgon/opendictate-desktop/releases/latest";

#[derive(Debug, Clone, Serialize)]
pub struct UpdateInfo {
    pub version: String,
    /// The release page.
    pub url: String,
    pub notes: Option<String>,
    /// This copy can download and install the update itself.
    pub can_install: bool,
}

#[derive(Debug, Clone, Serialize)]
struct Progress {
    downloaded: u64,
    total: Option<u64>,
}

/// Whether this copy of OpenDictate can replace itself.
fn self_updating() -> bool {
    #[cfg(target_os = "linux")]
    {
        std::env::var_os("APPIMAGE").is_some()
    }
    #[cfg(not(target_os = "linux"))]
    {
        true
    }
}

fn release_url(version: &str) -> String {
    format!("{RELEASES_URL}/tag/v{}", version.trim_start_matches('v'))
}

fn parse(v: &str) -> Option<(u64, u64, u64)> {
    let v = v.trim().trim_start_matches('v');
    let core = v.split(['-', '+']).next()?;
    let mut it = core.split('.').map(|p| p.parse::<u64>().ok());
    Some((it.next()??, it.next().flatten().unwrap_or(0), it.next().flatten().unwrap_or(0)))
}

pub fn is_newer(candidate: &str, current: &str) -> bool {
    matches!((parse(candidate), parse(current)), (Some(a), Some(b)) if a > b)
}

/// Returns the latest release if it is newer than this build. An error means GitHub could not be
/// asked (offline, not answering), which is not the same as "no update".
pub async fn check(app: &AppHandle, http: &reqwest::Client) -> Result<Option<UpdateInfo>> {
    if self_updating() {
        match app.updater() {
            Ok(updater) => match updater.check().await {
                Ok(Some(update)) => {
                    return Ok(Some(UpdateInfo {
                        url: release_url(&update.version),
                        version: update.version.clone(),
                        notes: update.body.clone().filter(|b| !b.trim().is_empty()),
                        can_install: true,
                    }))
                }
                Ok(None) => return Ok(None),
                // E.g. a release published before self-updating existed (no latest.json).
                Err(e) => log::info!("update check: {e}; asking the GitHub API instead"),
            },
            Err(e) => log::info!("updater unavailable: {e}"),
        }
    }
    latest_release(http).await
}

/// The plain "is there a newer release?" question, for copies that can't update themselves.
async fn latest_release(http: &reqwest::Client) -> Result<Option<UpdateInfo>> {
    let resp = http
        .get(LATEST_API_URL)
        .header("Accept", "application/vnd.github+json")
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| {
            log::info!("update check: {e}");
            anyhow!("Couldn't reach GitHub to check for updates. Check your internet connection.")
        })?;
    if !resp.status().is_success() {
        return Err(anyhow!("GitHub didn't answer the update check (HTTP {}). Try again later.", resp.status().as_u16()));
    }
    let unreadable = || anyhow!("GitHub's answer to the update check could not be read. Try again later.");
    let v: serde_json::Value = resp.json().await.map_err(|_| unreadable())?;
    let tag = v.get("tag_name").and_then(|t| t.as_str()).ok_or_else(unreadable)?;
    if !is_newer(tag, env!("CARGO_PKG_VERSION")) {
        return Ok(None);
    }
    let version = tag.trim_start_matches('v').to_string();
    Ok(Some(UpdateInfo {
        url: v.get("html_url").and_then(|u| u.as_str()).map(str::to_string).unwrap_or_else(|| release_url(&version)),
        notes: v.get("body").and_then(|b| b.as_str()).map(str::to_string).filter(|b| !b.trim().is_empty()),
        version,
        can_install: false,
    }))
}

/// Downloads, verifies and installs the latest release, then restarts OpenDictate.
/// Emits `od://update-progress` while downloading.
pub async fn install(app: AppHandle) -> Result<()> {
    if !self_updating() {
        return Err(anyhow!("This copy of OpenDictate can't update itself; download the new version from GitHub."));
    }
    let update = app.updater()?.check().await?.ok_or_else(|| anyhow!("OpenDictate is already up to date."))?;
    log::info!("downloading OpenDictate {}", update.version);
    let progress = app.clone();
    let mut downloaded = 0u64;
    let mut last_percent = u64::MAX;
    update
        .download_and_install(
            move |chunk, total| {
                downloaded += chunk as u64;
                // About a hundred events, not one per network chunk.
                let percent = total.map(|t| downloaded * 100 / t.max(1)).unwrap_or(downloaded >> 20);
                if percent != last_percent {
                    last_percent = percent;
                    let _ = progress.emit("od://update-progress", Progress { downloaded, total });
                }
            },
            || log::info!("update downloaded"),
        )
        .await?;
    log::info!("OpenDictate {} installed; restarting", update.version);
    app.restart();
}

#[cfg(test)]
mod tests {
    use super::{is_newer, release_url};

    #[test]
    fn compares_versions() {
        assert!(is_newer("v0.1.1", "0.1.0"));
        assert!(is_newer("v0.2.0", "0.1.9"));
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(!is_newer("v0.1.0", "0.1.0"));
        assert!(!is_newer("v0.0.9", "0.1.0"));
        assert!(!is_newer("garbage", "0.1.0"));
    }

    #[test]
    fn links_to_the_release_page() {
        assert_eq!(release_url("0.2.0"), "https://github.com/burakgon/opendictate-desktop/releases/tag/v0.2.0");
        assert_eq!(release_url("v0.2.0"), "https://github.com/burakgon/opendictate-desktop/releases/tag/v0.2.0");
    }
}
