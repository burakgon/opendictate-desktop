//! Lightweight "a new version is available" check against GitHub releases (no auto-install).

use std::time::Duration;

use serde::Serialize;

const LATEST_URL: &str = "https://api.github.com/repos/burakgon/opendictate-desktop/releases/latest";

#[derive(Debug, Clone, Serialize)]
pub struct UpdateInfo {
    pub version: String,
    pub url: String,
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

/// Returns the latest release if it is newer than this build.
pub async fn check(http: &reqwest::Client) -> Option<UpdateInfo> {
    let resp = http
        .get(LATEST_URL)
        .header("Accept", "application/vnd.github+json")
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let v: serde_json::Value = resp.json().await.ok()?;
    let tag = v.get("tag_name")?.as_str()?;
    let url = v.get("html_url").and_then(|u| u.as_str()).unwrap_or("https://github.com/burakgon/opendictate-desktop/releases/latest");
    is_newer(tag, env!("CARGO_PKG_VERSION")).then(|| UpdateInfo { version: tag.trim_start_matches('v').to_string(), url: url.to_string() })
}

#[cfg(test)]
mod tests {
    use super::is_newer;

    #[test]
    fn compares_versions() {
        assert!(is_newer("v0.1.1", "0.1.0"));
        assert!(is_newer("v0.2.0", "0.1.9"));
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(!is_newer("v0.1.0", "0.1.0"));
        assert!(!is_newer("v0.0.9", "0.1.0"));
        assert!(!is_newer("garbage", "0.1.0"));
    }
}
