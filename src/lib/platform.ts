/** True in the macOS app (known synchronously, before the backend reports the environment). */
export const isMac = /Mac/.test(navigator.platform) || /Macintosh/.test(navigator.userAgent);

/** Where OpenDictate lives when its window is closed. */
export const trayName = isMac ? "menu bar" : "system tray";
