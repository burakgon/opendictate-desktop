import { AlertCircle, Check, Wand2 } from "lucide-react";
import { StrictMode, useEffect, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { api, useEvent, type Status } from "./lib/api";
import "./styles.css";

const BARS = 26;

function Waveform({ command }: { command: boolean }) {
  const [levels, setLevels] = useState<number[]>(() => Array(BARS).fill(0));
  useEvent<number>("od://level", (lvl) => {
    const v = Math.min(1, Math.sqrt(lvl) * 3.4);
    setLevels((prev) => [...prev.slice(1), v]);
  });
  return (
    <div className="flex h-7 items-center gap-[3px]">
      {levels.map((v, i) => {
        // Taper the ends so the wave looks centred.
        const taper = Math.sin(((i + 1) / (BARS + 1)) * Math.PI) * 0.55 + 0.45;
        const h = Math.max(3, v * 28 * taper);
        return (
          <span
            key={i}
            className="w-[3px] rounded-full transition-[height] duration-75 ease-out"
            style={{
              height: h,
              background: command
                ? "linear-gradient(180deg, #f0abfc, #a855f7)"
                : "linear-gradient(180deg, #7dd3fc, #3b82f6)",
              opacity: 0.45 + v * 0.55,
            }}
          />
        );
      })}
    </div>
  );
}

function Timer({ since }: { since: number }) {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    const id = setInterval(() => setNow(Date.now()), 250);
    return () => clearInterval(id);
  }, []);
  const s = Math.max(0, Math.floor((now - since) / 1000));
  return (
    <span className="font-mono text-[12px] tabular-nums text-white/50">
      {Math.floor(s / 60)}:{String(s % 60).padStart(2, "0")}
    </span>
  );
}

function Overlay() {
  const [status, setStatus] = useState<Status | null>(null);
  const [visible, setVisible] = useState(false);
  const startedAt = useRef(Date.now());
  const lastSession = useRef(0);

  const apply = (s: Status) => {
    if (s.phase === "recording" && s.session !== lastSession.current) {
      lastSession.current = s.session;
      startedAt.current = Date.now();
    }
    setStatus(s);
    setVisible(s.phase !== "idle");
  };
  useEvent<Status>("od://status", apply);
  useEffect(() => {
    // The window may load after the first status event was sent.
    api.lastStatus().then((s) => s && apply(s));
    const onVis = () => document.visibilityState === "visible" && api.lastStatus().then((s) => s && apply(s));
    document.addEventListener("visibilitychange", onVis);
    return () => document.removeEventListener("visibilitychange", onVis);
  }, []);

  if (!status) return null;
  const { phase, mode } = status;
  const command = mode === "command" || mode === "action";
  const wide = phase === "done" || phase === "error" || (phase === "processing" && !!status.selection_chars);

  let icon: React.ReactNode;
  let body: React.ReactNode;
  let tail: React.ReactNode = null;

  switch (phase) {
    case "recording":
      icon = command ? (
        <div className="relative flex size-8 items-center justify-center rounded-full bg-fuchsia-500/20 text-fuchsia-300">
          <span className="absolute inset-0 animate-ping rounded-full bg-fuchsia-500/20" />
          <Wand2 className="size-4" />
        </div>
      ) : (
        <div className="relative flex size-8 items-center justify-center rounded-full bg-sky-500/15">
          <span className="absolute size-3 animate-ping rounded-full bg-sky-400/60" />
          <span className="relative size-3 rounded-full bg-sky-400 shadow-[0_0_12px_rgba(56,189,248,0.9)]" />
        </div>
      );
      body = <Waveform command={command} />;
      tail = (
        <div className="flex items-center gap-2">
          {command && <span className="rounded-full bg-fuchsia-500/15 px-2 py-0.5 text-[11px] font-semibold text-fuchsia-300">Command</span>}
          <Timer since={startedAt.current} />
        </div>
      );
      break;
    case "processing":
      icon = (
        <div className="relative size-8">
          <div className="absolute inset-0 rounded-full border-2 border-white/10" />
          <div
            className="absolute inset-0 animate-spin rounded-full border-2 border-transparent [animation-duration:800ms]"
            style={{ borderTopColor: command ? "#e879f9" : "#7dd3fc", borderRightColor: command ? "#a855f7" : "#3b82f6" }}
          />
        </div>
      );
      body = <span className="shimmer-text text-[14px] font-medium">{status.message ?? "Working"}…</span>;
      if (status.selection_chars)
        tail = <span className="rounded-full bg-white/[0.07] px-2 py-0.5 text-[11px] text-white/55">{status.selection_chars} chars selected</span>;
      break;
    case "done":
      icon = (
        <div className="flex size-8 items-center justify-center rounded-full bg-emerald-500/20 text-emerald-300">
          <Check className="size-4" strokeWidth={3} />
        </div>
      );
      body = <span className="line-clamp-1 text-[13.5px] text-white/85">{status.preview || status.message}</span>;
      tail = status.message === "Copied to clipboard" ? <span className="text-[11px] text-amber-300/80">copied</span> : null;
      break;
    case "error":
      icon = (
        <div className="flex size-8 shrink-0 items-center justify-center rounded-full bg-rose-500/20 text-rose-300">
          <AlertCircle className="size-4" />
        </div>
      );
      body = <span className="line-clamp-2 text-[12.5px] leading-snug text-rose-100/90">{status.message}</span>;
      break;
    default:
      icon = <div className="size-8 rounded-full bg-white/[0.06]" />;
      body = <span className="text-[13.5px] text-white/55">{status.message ?? "Cancelled"}</span>;
  }

  return (
    <div className="flex h-screen items-end justify-center pb-3">
      <div
        className="flex items-center gap-3 border border-white/[0.09] py-2 pl-2 pr-4 shadow-[0_12px_40px_rgba(0,0,0,0.55),inset_0_1px_0_rgba(255,255,255,0.06)] transition-all duration-300"
        style={{
          background: "linear-gradient(180deg, rgba(28,28,40,0.96), rgba(14,14,22,0.96))",
          borderRadius: phase === "error" ? 18 : 999,
          minWidth: 200,
          maxWidth: wide ? 456 : 340,
          opacity: visible ? 1 : 0,
          transform: visible ? "translateY(0) scale(1)" : "translateY(12px) scale(0.92)",
        }}
      >
        {icon}
        <div className="flex min-w-0 flex-1 items-center">{body}</div>
        {tail}
      </div>
    </div>
  );
}

createRoot(document.getElementById("overlay-root")!).render(
  <StrictMode>
    <Overlay />
  </StrictMode>,
);
