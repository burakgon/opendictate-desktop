//! Pauses music/video players while recording (Linux, via MPRIS) and resumes them afterwards.

use parking_lot::Mutex;

/// Players we paused, so only those are resumed.
static PAUSED: Mutex<Vec<String>> = Mutex::new(Vec::new());

#[cfg(target_os = "linux")]
async fn session() -> Option<zbus::Connection> {
    zbus::Connection::session().await.ok()
}

/// Pauses every MPRIS player that is currently playing.
pub async fn pause_playing() {
    #[cfg(target_os = "linux")]
    {
        let Some(conn) = session().await else { return };
        let Ok(dbus) = zbus::fdo::DBusProxy::new(&conn).await else { return };
        let Ok(names) = dbus.list_names().await else { return };
        let mut paused = Vec::new();
        for name in names.iter().map(|n| n.to_string()).filter(|n| n.starts_with("org.mpris.MediaPlayer2.")) {
            let Ok(player) = zbus::Proxy::new(&conn, name.as_str(), "/org/mpris/MediaPlayer2", "org.mpris.MediaPlayer2.Player").await else {
                continue;
            };
            let playing = player.get_property::<String>("PlaybackStatus").await.map(|s| s == "Playing").unwrap_or(false);
            if playing && player.call::<_, _, ()>("Pause", &()).await.is_ok() {
                log::info!("paused media player {name}");
                paused.push(name);
            }
        }
        PAUSED.lock().extend(paused);
    }
}

/// Resumes the players paused by [`pause_playing`].
pub async fn resume_paused() {
    let names: Vec<String> = std::mem::take(&mut *PAUSED.lock());
    #[cfg(target_os = "linux")]
    {
        if names.is_empty() {
            return;
        }
        let Some(conn) = session().await else { return };
        for name in names {
            if let Ok(player) = zbus::Proxy::new(&conn, name.as_str(), "/org/mpris/MediaPlayer2", "org.mpris.MediaPlayer2.Player").await {
                let _ = player.call::<_, _, ()>("Play", &()).await;
            }
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = names;
}
