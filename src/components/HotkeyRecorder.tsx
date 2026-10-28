import { Keyboard, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { api } from "../lib/api";
import { buildAccelerator, codeToToken } from "../lib/keys";
import { Button, Keycaps, cx } from "./ui";

const MOUSE: Record<number, string> = { 1: "MouseMiddle", 3: "Mouse4", 4: "Mouse5" };

export function HotkeyRecorder({
  value,
  onChange,
  os,
  allowMouse,
}: {
  value: string;
  onChange: (accel: string) => void;
  os?: string;
  allowMouse?: boolean;
}) {
  const [recording, setRecording] = useState(false);
  const [live, setLive] = useState<string[]>([]);
  const down = useRef(new Set<string>());
  const all = useRef<string[]>([]);

  useEffect(() => {
    if (!recording) return;
    api.pauseHotkeys(true).catch(() => {});
    down.current = new Set();
    all.current = [];
    setLive([]);

    const finish = (accel: string | null) => {
      setRecording(false);
      if (accel) onChange(accel);
    };
    const onDown = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.repeat) return;
      const t = codeToToken(e.code);
      if (!t) return;
      down.current.add(t);
      if (!all.current.includes(t)) all.current.push(t);
      setLive([...all.current]);
    };
    const onUp = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      const t = codeToToken(e.code);
      if (t) down.current.delete(t);
      if (down.current.size === 0 && all.current.length > 0) {
        if (all.current.length === 1 && all.current[0] === "Escape") return finish(null);
        finish(buildAccelerator(all.current));
      }
    };
    const onMouse = (e: MouseEvent) => {
      const t = MOUSE[e.button];
      if (!t || !allowMouse) return;
      e.preventDefault();
      finish(buildAccelerator([...down.current, t]));
    };
    window.addEventListener("keydown", onDown, true);
    window.addEventListener("keyup", onUp, true);
    window.addEventListener("mousedown", onMouse, true);
    return () => {
      window.removeEventListener("keydown", onDown, true);
      window.removeEventListener("keyup", onUp, true);
      window.removeEventListener("mousedown", onMouse, true);
      api.pauseHotkeys(false).catch(() => {});
    };
  }, [recording, onChange, allowMouse]);

  return (
    <div className="flex items-center gap-2">
      <div
        className={cx(
          "flex h-10 min-w-48 items-center justify-center rounded-xl border px-3 transition",
          recording ? "border-brand-400/60 bg-brand-500/10 ring-4 ring-brand-500/10" : "border-white/10 bg-black/25",
        )}
      >
        {recording ? (
          live.length ? (
            <Keycaps accel={live.join("+")} os={os} size="sm" />
          ) : (
            <span className="animate-pulse text-[13px] text-brand-300">Press your shortcut…</span>
          )
        ) : (
          <Keycaps accel={value} os={os} size="sm" />
        )}
      </div>
      {recording ? (
        <Button size="sm" variant="ghost" icon={<X className="size-3.5" />} onClick={() => setRecording(false)}>
          Cancel
        </Button>
      ) : (
        <Button size="sm" variant="secondary" icon={<Keyboard className="size-3.5" />} onClick={() => setRecording(true)}>
          Change
        </Button>
      )}
    </div>
  );
}
