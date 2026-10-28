import { AlertTriangle, ArrowRight, Clock, Gauge, MessageSquareText, Mic, RotateCcw, Sparkles, Wand2, Zap } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { api, useEvent, type HistoryEntry, type Stats, type Status } from "../lib/api";
import { sameAccelerator } from "../lib/keys";
import { hasKey, useStore } from "../lib/store";
import { Button, Keycaps, TextArea, cx } from "../components/ui";
import { LevelBars, StatusOrb } from "../components/Visuals";

function formatDuration(secs: number) {
  if (secs < 60) return `${Math.round(secs)}s`;
  const m = Math.floor(secs / 60);
  if (m < 60) return `${m}m`;
  const h = Math.floor(m / 60);
  return `${h}h ${m % 60}m`;
}

export function timeAgo(ts: number) {
  const s = Math.floor((Date.now() - ts) / 1000);
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.floor(s / 60)}m ago`;
  if (s < 86400) return `${Math.floor(s / 3600)}h ago`;
  return new Date(ts).toLocaleDateString();
}

export function Home({ status, navigate }: { status: Status | null; navigate: (p: string) => void }) {
  const { settings, system, providers } = useStore();
  const [stats, setStats] = useState<Stats | null>(null);
  const [recent, setRecent] = useState<HistoryEntry[]>([]);
  const [playground, setPlayground] = useState("");
  const os = system?.environment.os;

  const load = () => {
    api.stats().then(setStats);
    api.history().then((h) => setRecent(h.slice(0, 4)));
  };
  useEffect(load, []);
  useEvent("od://history", load);

  const shared = settings.command.enabled && sameAccelerator(settings.hotkeys.dictate, settings.hotkeys.command);
  const issues = useMemo(() => {
    const list: { text: string; action: string; page: string }[] = [];
    if (!hasKey(settings, providers, settings.dictation.stt.provider))
      list.push({ text: "Add an API key so OpenDictate can transcribe your voice.", action: "Add key", page: "providers" });
    if (system && !system.injection_ok)
      list.push({ text: "OpenDictate can't paste into other apps yet.", action: "Fix", page: "settings" });
    if (system && !system.hotkeys.ok)
      list.push({ text: `Global hotkey problem: ${system.hotkeys.message}`, action: "Open", page: "settings" });
    return list;
  }, [settings, providers, system]);

  const timeSaved = stats ? Math.max(0, (stats.words / 40) * 60 - stats.audio_secs) : 0;
  const sessions = stats ? stats.dictations + stats.commands : 0;
  const avgLatency = stats && sessions ? stats.latency_ms_total / sessions / 1000 : 0;

  const phase = status?.phase ?? "idle";
  const live = phase === "recording" || phase === "processing";
  const headline =
    phase === "recording"
      ? status?.mode === "command"
        ? "Listening for your command…"
        : "Listening…"
      : phase === "processing"
        ? status?.message ?? "Working…"
        : "Ready when you are";

  return (
    <div className="animate-fade-in">
      {status?.phase === "error" && (
        <div className="mb-5 flex items-center gap-3 rounded-xl border border-rose-500/20 bg-rose-500/[0.07] px-4 py-3">
          <AlertTriangle className="size-4 shrink-0 text-rose-300" />
          <div className="min-w-0 flex-1 text-[13.5px] text-rose-100/90">
            <span className="line-clamp-2">{status.message}</span>
          </div>
          {status.retryable && (
            <Button size="sm" variant="secondary" icon={<RotateCcw className="size-3.5" />} onClick={() => api.trigger("retry")}>
              Retry
            </Button>
          )}
        </div>
      )}
      {issues.length > 0 && (
        <div className="mb-5 space-y-2">
          {issues.map((i) => (
            <div key={i.text} className="flex items-center gap-3 rounded-xl border border-amber-500/20 bg-amber-500/[0.07] px-4 py-3">
              <AlertTriangle className="size-4 shrink-0 text-amber-300" />
              <div className="flex-1 text-[13.5px] text-amber-100/90">{i.text}</div>
              <Button size="sm" variant="secondary" onClick={() => navigate(i.page)}>
                {i.action}
              </Button>
            </div>
          ))}
        </div>
      )}

      {/* Hero */}
      <section className="card relative mb-5 overflow-hidden p-8">
        <div className="pointer-events-none absolute -right-24 -top-24 size-80 rounded-full bg-brand-500/20 blur-3xl" />
        <div className="pointer-events-none absolute -bottom-32 left-40 size-72 rounded-full bg-accent-500/10 blur-3xl" />
        <div className="relative flex items-center gap-8">
          <div className="-m-6 shrink-0">
            <StatusOrb status={status} size={96} />
          </div>
          <div className="min-w-0 flex-1">
            <div className={cx("text-[13px] font-medium uppercase tracking-[0.14em]", live ? "text-brand-300" : "text-white/40")}>
              {live ? (status?.mode === "command" ? "Voice command" : "Dictation") : "OpenDictate"}
            </div>
            <h2 className="mt-1.5 text-[30px] font-semibold tracking-tight text-white">{headline}</h2>
            {phase === "recording" ? (
              <LevelBars className="mt-4 h-12" bars={48} />
            ) : (
              <div className="mt-4 space-y-2.5 text-[14px] text-white/60">
                {shared ? (
                  <>
                    <div className="flex items-center gap-3">
                      <Keycaps accel={settings.hotkeys.dictate} os={os} />
                      <span>
                        <b className="font-medium text-white/85">Tap</b> to dictate, tap again to finish
                      </span>
                    </div>
                    <div className="flex items-center gap-3">
                      <Keycaps accel={settings.hotkeys.command} os={os} />
                      <span>
                        <b className="font-medium text-white/85">Hold</b> or <b className="font-medium text-white/85">double-tap</b> to speak a command
                      </span>
                    </div>
                  </>
                ) : (
                  <>
                    <div className="flex items-center gap-3">
                      <Keycaps accel={settings.hotkeys.dictate} os={os} />
                      <span>Dictate anywhere</span>
                    </div>
                    {settings.command.enabled && (
                      <div className="flex items-center gap-3">
                        <Keycaps accel={settings.hotkeys.command} os={os} />
                        <span>Voice command on selected text</span>
                      </div>
                    )}
                  </>
                )}
              </div>
            )}
          </div>
          <div className="flex shrink-0 flex-col gap-2">
            <Button variant="primary" icon={<Mic className="size-4" />} onClick={() => api.trigger("dictate")}>
              {phase === "recording" ? "Stop" : "Dictate"}
            </Button>
            {settings.command.enabled && (
              <Button variant="secondary" icon={<Wand2 className="size-4" />} onClick={() => api.trigger("command")}>
                Command
              </Button>
            )}
          </div>
        </div>
      </section>

      {/* Stats */}
      <div className="mb-5 grid grid-cols-4 gap-4">
        {[
          { icon: <MessageSquareText className="size-4" />, label: "Words dictated", value: (stats?.words ?? 0).toLocaleString() },
          { icon: <Clock className="size-4" />, label: "Time saved", value: formatDuration(timeSaved), hint: "vs. typing at 40 wpm" },
          { icon: <Zap className="size-4" />, label: "Sessions", value: sessions.toLocaleString() },
          { icon: <Gauge className="size-4" />, label: "Avg. turnaround", value: avgLatency ? `${avgLatency.toFixed(1)}s` : "—" },
        ].map((s) => (
          <div key={s.label} className="card p-5" title={s.hint}>
            <div className="flex items-center gap-2 text-[12.5px] text-white/45">
              <span className="text-brand-300">{s.icon}</span>
              {s.label}
            </div>
            <div className="mt-2 text-[26px] font-semibold tracking-tight text-white">{s.value}</div>
          </div>
        ))}
      </div>

      <div className="grid grid-cols-5 gap-5">
        {/* Playground */}
        <section className="card col-span-3 p-6">
          <div className="mb-3 flex items-center justify-between">
            <div>
              <h3 className="text-[15px] font-semibold text-white">Try it here</h3>
              <p className="mt-0.5 text-[13px] text-white/45">
                Click in the box, press your shortcut and speak. Then select some text and hold to give a command.
              </p>
            </div>
            {playground && (
              <Button size="sm" variant="ghost" onClick={() => setPlayground("")}>
                Clear
              </Button>
            )}
          </div>
          <TextArea
            value={playground}
            onChange={(e) => setPlayground(e.target.value)}
            rows={7}
            placeholder="Your words will appear here…"
            className="min-h-40 text-[14.5px]"
          />
        </section>

        {/* Recent */}
        <section className="card col-span-2 flex flex-col p-6">
          <div className="mb-3 flex items-center justify-between">
            <h3 className="text-[15px] font-semibold text-white">Recent</h3>
            <button className="flex items-center gap-1 text-[12.5px] text-white/45 hover:text-white" onClick={() => navigate("history")}>
              All history <ArrowRight className="size-3.5" />
            </button>
          </div>
          {recent.length === 0 ? (
            <div className="flex flex-1 flex-col items-center justify-center gap-2 py-6 text-center text-[13px] text-white/35">
              <Sparkles className="size-5 text-white/25" />
              Your dictations will show up here.
            </div>
          ) : (
            <div className="space-y-2">
              {recent.map((e) => (
                <button
                  key={e.id}
                  className="block w-full rounded-xl border border-white/[0.05] bg-white/[0.02] px-3.5 py-2.5 text-left transition hover:bg-white/[0.05]"
                  onClick={() => api.copy(e.text)}
                  title="Click to copy"
                >
                  <div className="mb-0.5 flex items-center gap-2 text-[11.5px] text-white/35">
                    <span className={cx("font-medium", e.kind === "dictation" ? "text-accent-400/80" : "text-brand-300/90")}>
                      {e.kind === "dictation" ? "Dictation" : e.kind === "command" ? "Command" : "Action"}
                    </span>
                    · {timeAgo(e.ts)}
                  </div>
                  <div className="line-clamp-2 text-[13px] leading-snug text-white/80">{e.text}</div>
                </button>
              ))}
            </div>
          )}
        </section>
      </div>
    </div>
  );
}
