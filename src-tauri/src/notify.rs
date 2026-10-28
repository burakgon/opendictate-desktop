//! Desktop notifications, used for errors when the overlay can't be shown.

pub fn send(summary: String, body: String) {
    #[cfg(target_os = "linux")]
    tauri::async_runtime::spawn(async move {
        let Ok(conn) = zbus::Connection::session().await else { return };
        let Ok(proxy) = zbus::Proxy::new(&conn, "org.freedesktop.Notifications", "/org/freedesktop/Notifications", "org.freedesktop.Notifications").await else {
            return;
        };
        let hints: std::collections::HashMap<&str, zbus::zvariant::Value> = std::collections::HashMap::new();
        let _: Result<u32, _> = proxy
            .call("Notify", &("OpenDictate", 0u32, "io.github.burakgon.OpenDictate", summary.as_str(), body.as_str(), Vec::<&str>::new(), hints, 6000i32))
            .await;
    });
    #[cfg(not(target_os = "linux"))]
    let _ = (summary, body);
}
