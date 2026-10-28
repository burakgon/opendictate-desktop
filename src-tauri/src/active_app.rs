//! Which application has keyboard focus? Used to pick the right paste/copy shortcut:
//! terminals paste with Ctrl+Shift+V, and Ctrl+C would interrupt the running program.
//!
//! * KDE Plasma (Wayland & X11): a one-shot KWin script reports the active window over D-Bus.
//! * Hyprland: `hyprctl activewindow -j`. Sway: `swaymsg -t get_tree`.
//! * X11: `xprop` on `_NET_ACTIVE_WINDOW`.
//! * GNOME on Wayland exposes no API for this; we fall back to "unknown".

#[cfg(target_os = "linux")]
mod linux {
    use std::{process::Command, sync::OnceLock, time::Duration};

    use parking_lot::Mutex;
    use tokio::sync::Notify;

    const BUS_NAME: &str = "io.github.burakgon.OpenDictate.Focus";
    const OBJ_PATH: &str = "/Focus";
    const IFACE: &str = "io.github.burakgon.OpenDictate.Focus";

    struct Reporter {
        last: Mutex<Option<(String, String)>>,
        notify: Notify,
    }

    struct FocusIface {
        reporter: &'static Reporter,
    }

    #[zbus::interface(name = "io.github.burakgon.OpenDictate.Focus")]
    impl FocusIface {
        fn report(&self, token: String, app: String) {
            *self.reporter.last.lock() = Some((token, app));
            self.reporter.notify.notify_waiters();
        }
    }

    fn reporter() -> &'static Reporter {
        static R: OnceLock<Reporter> = OnceLock::new();
        R.get_or_init(|| Reporter { last: Mutex::new(None), notify: Notify::new() })
    }

    async fn connection() -> Option<zbus::Connection> {
        static CONN: tokio::sync::OnceCell<Option<zbus::Connection>> = tokio::sync::OnceCell::const_new();
        CONN.get_or_init(|| async {
            let r = zbus::connection::Builder::session()
                .ok()?
                .name(BUS_NAME)
                .ok()?
                .serve_at(OBJ_PATH, FocusIface { reporter: reporter() })
                .ok()?
                .build()
                .await;
            match r {
                Ok(c) => Some(c),
                Err(e) => {
                    log::warn!("focus detection: D-Bus unavailable: {e}");
                    None
                }
            }
        })
        .await
        .clone()
    }

    fn is_kde() -> bool {
        std::env::var("XDG_CURRENT_DESKTOP").map(|d| d.to_uppercase().contains("KDE")).unwrap_or(false)
    }

    async fn kwin_active() -> Option<String> {
        let conn = connection().await?;
        let token = crate::history::now_ms().to_string();
        let script = format!(
            "const w = workspace.activeWindow;\n\
             callDBus(\"{BUS_NAME}\", \"{OBJ_PATH}\", \"{IFACE}\", \"Report\", \"{token}\", \
             w ? String(w.desktopFileName || w.resourceClass || \"\") : \"\");\n"
        );
        let path = std::env::temp_dir().join(format!("opendictate-focus-{}.js", std::process::id()));
        std::fs::write(&path, script).ok()?;
        let name = format!("opendictate-focus-{token}");

        let notified = reporter().notify.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();

        let scripting = zbus::Proxy::new(&conn, "org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting").await.ok()?;
        let id: i32 = scripting.call("loadScript", &(path.to_string_lossy().to_string(), name.as_str())).await.ok()?;
        let script_path = format!("/Scripting/Script{id}");
        if let Ok(script) = zbus::Proxy::new(&conn, "org.kde.KWin", script_path.as_str(), "org.kde.kwin.Script").await {
            let _: Result<(), _> = script.call("run", &()).await;
        }
        let got = tokio::time::timeout(Duration::from_millis(400), async {
            loop {
                if let Some((t, app)) = reporter().last.lock().as_ref() {
                    if *t == token {
                        return app.clone();
                    }
                }
                notified.as_mut().await;
                notified.set(reporter().notify.notified());
                notified.as_mut().enable();
            }
        })
        .await
        .ok();
        let _: Result<bool, _> = scripting.call("unloadScript", &(name.as_str(),)).await;
        let _ = std::fs::remove_file(&path);
        got
    }

    fn run(cmd: &str, args: &[&str]) -> Option<String> {
        let out = Command::new(cmd).args(args).output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).to_string())
    }

    fn hyprland_active() -> Option<String> {
        let json: serde_json::Value = serde_json::from_str(&run("hyprctl", &["activewindow", "-j"])?).ok()?;
        json.get("class").and_then(|c| c.as_str()).map(str::to_string)
    }

    fn sway_active() -> Option<String> {
        fn find(node: &serde_json::Value) -> Option<String> {
            if node.get("focused").and_then(|f| f.as_bool()) == Some(true) {
                return node
                    .get("app_id")
                    .and_then(|a| a.as_str())
                    .or_else(|| node.pointer("/window_properties/class").and_then(|c| c.as_str()))
                    .map(str::to_string);
            }
            for key in ["nodes", "floating_nodes"] {
                if let Some(children) = node.get(key).and_then(|n| n.as_array()) {
                    for c in children {
                        if let Some(found) = find(c) {
                            return Some(found);
                        }
                    }
                }
            }
            None
        }
        let tree: serde_json::Value = serde_json::from_str(&run("swaymsg", &["-t", "get_tree", "-r"])?).ok()?;
        find(&tree)
    }

    fn x11_active() -> Option<String> {
        let root = run("xprop", &["-root", "_NET_ACTIVE_WINDOW"])?;
        let id = root.split_whitespace().last()?.trim().to_string();
        let class = run("xprop", &["-id", &id, "WM_CLASS"])?;
        // WM_CLASS(STRING) = "konsole", "konsole"
        class.split('"').nth(3).or_else(|| class.split('"').nth(1)).map(str::to_string)
    }

    pub async fn active_app() -> Option<String> {
        let app = if is_kde() {
            kwin_active().await
        } else if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some() {
            tokio::task::spawn_blocking(hyprland_active).await.ok().flatten()
        } else if std::env::var_os("SWAYSOCK").is_some() {
            tokio::task::spawn_blocking(sway_active).await.ok().flatten()
        } else if std::env::var_os("WAYLAND_DISPLAY").is_none() && std::env::var_os("DISPLAY").is_some() {
            tokio::task::spawn_blocking(x11_active).await.ok().flatten()
        } else {
            None
        };
        app.filter(|a| !a.is_empty())
    }
}

/// The focused application's id/class (lower-cased), if it can be determined.
pub async fn active_app() -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        linux::active_app().await.map(|a| a.to_lowercase())
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

const TERMINALS: &[&str] = &[
    "konsole", "yakuake", "gnome-terminal", "gnome-terminal-server", "kgx", "console", "ptyxis", "xterm", "uxterm",
    "urxvt", "rxvt", "alacritty", "kitty", "foot", "footclient", "wezterm", "wezterm-gui", "ghostty", "tilix",
    "terminator", "xfce4-terminal", "lxterminal", "mate-terminal", "qterminal", "st", "st-256color", "guake", "tilda",
    "warp", "dev.warp.warp", "tabby", "hyper", "cool-retro-term", "blackbox", "contour", "rio", "termite", "sakura",
    "terminology", "deepin-terminal", "cosmic-term", "waveterm", "wave", "kermit", "zutty", "terminal",
];

/// True for terminal emulators, given an app id like `org.kde.konsole` or `com.mitchellh.ghostty`.
pub fn is_terminal(app: &str) -> bool {
    let app = app.to_lowercase();
    let app = app.trim_end_matches(".desktop");
    let last = app.rsplit('.').next().unwrap_or(app);
    TERMINALS.contains(&app) || TERMINALS.contains(&last) || last.ends_with("terminal") || last.ends_with("-term")
}

#[cfg(test)]
mod tests {
    use super::is_terminal;

    #[test]
    fn detects_terminals() {
        for t in ["org.kde.konsole", "com.mitchellh.ghostty", "kitty", "Alacritty", "org.gnome.Ptyxis", "org.wezfurlong.wezterm", "io.elementary.terminal", "xfce4-terminal"] {
            assert!(is_terminal(t), "{t}");
        }
        for t in ["firefox", "org.kde.kate", "code", "brave-browser", "org.telegram.desktop", "vivaldi-stable", "obsidian"] {
            assert!(!is_terminal(t), "{t}");
        }
    }
}
