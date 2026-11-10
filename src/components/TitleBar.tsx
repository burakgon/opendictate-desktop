import { getCurrentWindow } from "@tauri-apps/api/window";
import { Minus, Square, X, Copy } from "lucide-react";
import { useEffect, useState } from "react";
import { useStore } from "../lib/store";

export function TitleBar({ os }: { os?: string }) {
  const { settings } = useStore();
  const [maximized, setMaximized] = useState(false);
  useEffect(() => {
    const w = getCurrentWindow();
    const sync = () => w.isMaximized().then((m) => {
      setMaximized(m);
      document.documentElement.classList.toggle("maximized", m);
    });
    sync();
    const un = w.onResized(sync);
    return () => {
      un.then((f) => f());
    };
  }, []);
  const w = getCurrentWindow();
  const isMac = os === "macos";

  return (
    // Double-clicking a drag region already zooms the window on macOS.
    <div data-tauri-drag-region className="flex h-11 shrink-0 items-center justify-end px-2" onDoubleClick={isMac ? undefined : () => w.toggleMaximize()}>
      {!isMac && (
        <div className="flex items-center gap-0.5">
          <button
            className="flex size-8 items-center justify-center rounded-lg text-white/45 transition hover:bg-white/[0.08] hover:text-white"
            onClick={() => w.minimize()}
            title="Minimize"
          >
            <Minus className="size-4" />
          </button>
          <button
            className="flex size-8 items-center justify-center rounded-lg text-white/45 transition hover:bg-white/[0.08] hover:text-white"
            onClick={() => w.toggleMaximize()}
            title={maximized ? "Restore" : "Maximize"}
          >
            {maximized ? <Copy className="size-3.5 -scale-x-100" /> : <Square className="size-3.5" />}
          </button>
          <button
            className="flex size-8 items-center justify-center rounded-lg text-white/45 transition hover:bg-rose-500/80 hover:text-white"
            onClick={() => w.close()}
            title={settings.general.show_tray_icon ? "Close to tray" : "Close (OpenDictate keeps running)"}
          >
            <X className="size-4" />
          </button>
        </div>
      )}
    </div>
  );
}
