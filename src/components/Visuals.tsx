import { useEffect, useRef, useState } from "react";
import { useEvent, type Status } from "../lib/api";
import { cx } from "./ui";

/** A row of bars driven by a stream of RMS levels. */
export function LevelBars({
  event = "od://level",
  bars = 28,
  className,
  color = "from-brand-400 to-accent-400",
  active = true,
}: {
  event?: string;
  bars?: number;
  className?: string;
  color?: string;
  active?: boolean;
}) {
  const [levels, setLevels] = useState<number[]>(() => Array(bars).fill(0));
  useEvent<number>(event, (lvl) => {
    if (!active) return;
    const v = Math.min(1, Math.sqrt(lvl) * 3.2);
    setLevels((prev) => [...prev.slice(1), v]);
  });
  useEffect(() => {
    if (!active) setLevels(Array(bars).fill(0));
  }, [active, bars]);
  return (
    <div className={cx("flex h-10 items-center gap-[3px]", className)}>
      {levels.map((v, i) => (
        <div
          key={i}
          className={cx("w-[3px] rounded-full bg-gradient-to-t transition-[height] duration-75", color)}
          style={{ height: `${Math.max(8, v * 100)}%`, opacity: 0.35 + v * 0.65 }}
        />
      ))}
    </div>
  );
}

/** Big animated orb on the home screen reflecting the engine state. */
export function StatusOrb({ status, size = 120 }: { status: Status | null; size?: number }) {
  const [level, setLevel] = useState(0);
  const smooth = useRef(0);
  useEvent<number>("od://level", (l) => {
    smooth.current = smooth.current * 0.6 + Math.min(1, Math.sqrt(l) * 3) * 0.4;
    setLevel(smooth.current);
  });
  const phase = status?.phase ?? "idle";
  const recording = phase === "recording";
  const processing = phase === "processing";
  const command = status?.mode === "command" || status?.mode === "action";
  useEffect(() => {
    if (!recording) {
      smooth.current = 0;
      setLevel(0);
    }
  }, [recording]);

  const gradient = command
    ? "radial-gradient(circle at 30% 25%, #f0abfc, #a855f7 45%, #6d28d9 80%)"
    : "radial-gradient(circle at 30% 25%, #bae6fd, #3b82f6 45%, #4f46e5 90%)";
  return (
    <div className="relative flex items-center justify-center" style={{ width: size * 1.8, height: size * 1.8 }}>
      {recording && (
        <>
          <span className="absolute rounded-full bg-brand-500/25 animate-pulse-ring" style={{ width: size, height: size }} />
          <span
            className="absolute rounded-full bg-accent-500/20 animate-pulse-ring"
            style={{ width: size, height: size, animationDelay: "0.8s" }}
          />
        </>
      )}
      <div
        className={cx("relative rounded-full transition-transform duration-100", !recording && !processing && "animate-float")}
        style={{
          width: size,
          height: size,
          background: gradient,
          transform: `scale(${1 + level * 0.28})`,
          boxShadow: `0 0 ${40 + level * 80}px ${command ? "rgba(168,85,247,0.55)" : "rgba(59,130,246,0.5)"}, inset 0 -10px 30px rgba(0,0,0,0.25), inset 0 8px 20px rgba(255,255,255,0.25)`,
        }}
      >
        {processing && (
          <div className="absolute inset-[-6px] animate-spin rounded-full border-2 border-transparent border-t-white/70 [animation-duration:900ms]" />
        )}
      </div>
    </div>
  );
}
