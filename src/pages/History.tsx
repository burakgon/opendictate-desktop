import { Copy, History as HistoryIcon, Search, Trash2 } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { Button, IconButton, PageHeader, Segmented, cx, useToast } from "../components/ui";
import { api, useEvent, type HistoryEntry } from "../lib/api";
import { useStore } from "../lib/store";
import { timeAgo } from "./Home";

type Filter = "all" | "dictation" | "command";

function dayLabel(ts: number) {
  const d = new Date(ts);
  const today = new Date();
  const yesterday = new Date();
  yesterday.setDate(today.getDate() - 1);
  if (d.toDateString() === today.toDateString()) return "Today";
  if (d.toDateString() === yesterday.toDateString()) return "Yesterday";
  return d.toLocaleDateString(undefined, { weekday: "long", month: "long", day: "numeric" });
}

export function History() {
  const { settings } = useStore();
  const [entries, setEntries] = useState<HistoryEntry[]>([]);
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<Filter>("all");
  const [confirm, setConfirm] = useState(false);
  const toast = useToast();

  const load = () => api.history().then(setEntries);
  useEffect(() => {
    load();
  }, []);
  useEvent("od://history", load);

  const groups = useMemo(() => {
    const q = query.trim().toLowerCase();
    const list = entries.filter((e) => {
      if (filter === "dictation" && e.kind !== "dictation") return false;
      if (filter === "command" && e.kind === "dictation") return false;
      if (!q) return true;
      return e.text.toLowerCase().includes(q) || e.transcript.toLowerCase().includes(q);
    });
    const out: { day: string; items: HistoryEntry[] }[] = [];
    for (const e of list) {
      const day = dayLabel(e.ts);
      const last = out[out.length - 1];
      if (last && last.day === day) last.items.push(e);
      else out.push({ day, items: [e] });
    }
    return out;
  }, [entries, query, filter]);

  return (
    <div>
      <PageHeader
        title="History"
        description={settings.general.history_enabled ? "Everything you've dictated, stored only on this computer." : "History is turned off in Settings."}
        actions={
          entries.length > 0 &&
          (confirm ? (
            <>
              <Button size="sm" variant="ghost" onClick={() => setConfirm(false)}>
                Keep
              </Button>
              <Button
                size="sm"
                variant="danger"
                onClick={async () => {
                  await api.clearHistory();
                  setConfirm(false);
                  load();
                }}
              >
                Delete all
              </Button>
            </>
          ) : (
            <Button size="sm" variant="ghost" icon={<Trash2 className="size-3.5" />} onClick={() => setConfirm(true)}>
              Clear history
            </Button>
          ))
        }
      />

      <div className="mb-5 flex items-center gap-3">
        <div className="relative flex-1">
          <Search className="pointer-events-none absolute left-3.5 top-1/2 size-4 -translate-y-1/2 text-white/30" />
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search your words…"
            className="h-10 w-full rounded-xl border border-white/10 bg-black/25 pl-10 pr-3 text-[14px] text-white outline-none placeholder:text-white/25 focus:border-brand-400/60"
          />
        </div>
        <Segmented<Filter>
          value={filter}
          onChange={setFilter}
          options={[
            { value: "all", label: "All" },
            { value: "dictation", label: "Dictations" },
            { value: "command", label: "Commands" },
          ]}
        />
      </div>

      {groups.length === 0 ? (
        <div className="card flex flex-col items-center justify-center gap-3 py-20 text-center">
          <HistoryIcon className="size-8 text-white/20" />
          <div className="text-[14px] text-white/45">{query ? "Nothing matches your search." : "Nothing here yet — start dictating!"}</div>
        </div>
      ) : (
        groups.map((g) => (
          <div key={g.day} className="mb-6">
            <div className="mb-2 px-1 text-[12px] font-semibold uppercase tracking-wider text-white/35">{g.day}</div>
            <div className="card divide-y divide-white/[0.05] overflow-hidden">
              {g.items.map((e) => (
                <div key={e.id} className="group flex gap-4 px-5 py-4 transition hover:bg-white/[0.02]">
                  <div className="w-16 shrink-0 pt-0.5 text-[12px] text-white/35" title={new Date(e.ts).toLocaleString()}>
                    {new Date(e.ts).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" })}
                  </div>
                  <div className="min-w-0 flex-1">
                    {e.kind !== "dictation" && (
                      <div className="mb-1.5 flex items-center gap-2 text-[12.5px]">
                        <span className="rounded-md bg-brand-500/15 px-1.5 py-0.5 text-[11px] font-medium text-brand-300">
                          {e.kind === "command" ? "Command" : "Action"}
                        </span>
                        <span className="italic text-white/55">“{e.transcript}”</span>
                        {e.selection && <span className="text-white/30">· on {e.selection.length} chars</span>}
                      </div>
                    )}
                    <div className="selectable whitespace-pre-wrap text-[14px] leading-relaxed text-white/85">{e.text}</div>
                    <div className="mt-1.5 flex flex-wrap gap-x-3 text-[11.5px] text-white/30">
                      <span>{timeAgo(e.ts)}</span>
                      {e.audio_secs > 0 && <span>{e.audio_secs.toFixed(1)}s audio</span>}
                      <span>{(e.latency_ms / 1000).toFixed(1)}s turnaround</span>
                      <span>{e.words} words</span>
                      {e.stt_model && <span className="font-mono">{e.stt_model}</span>}
                      {e.llm_model && <span className="font-mono">{e.llm_model}</span>}
                      {!e.inserted && <span className="text-amber-300/70">copied to clipboard</span>}
                    </div>
                  </div>
                  <div className={cx("flex shrink-0 items-start gap-0.5 opacity-0 transition group-hover:opacity-100")}>
                    <IconButton
                      title="Copy"
                      onClick={async () => {
                        await api.copy(e.text);
                        toast("Copied to clipboard", "success");
                      }}
                    >
                      <Copy className="size-4" />
                    </IconButton>
                    <IconButton
                      title="Delete"
                      className="hover:text-rose-300"
                      onClick={async () => {
                        await api.deleteHistory(e.id);
                        load();
                      }}
                    >
                      <Trash2 className="size-4" />
                    </IconButton>
                  </div>
                </div>
              ))}
            </div>
          </div>
        ))
      )}
    </div>
  );
}
