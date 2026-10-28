import { AlertTriangle, ChevronDown, Loader2, RefreshCw, Search } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { api, errorText, type ModelInfo, type ModelRef, type ProviderId } from "../lib/api";
import { LLM_SUGGESTIONS, STT_SUGGESTIONS, TAG_STYLE } from "../lib/catalog";
import { hasKey, useStore } from "../lib/store";
import { Badge, Select, cx, inputClass } from "./ui";

const cache = new Map<string, ModelInfo[]>();

export function ModelPicker({
  kind,
  value,
  onChange,
  onNavigate,
}: {
  kind: "stt" | "llm";
  value: ModelRef;
  onChange: (v: ModelRef) => void;
  onNavigate?: (page: string) => void;
}) {
  const { settings, providers } = useStore();
  const usable = providers.filter((p) => (kind === "stt" ? p.stt !== "none" : p.llm));
  const keyed = hasKey(settings, providers, value.provider);
  const suggestions = (kind === "stt" ? STT_SUGGESTIONS : LLM_SUGGESTIONS)[value.provider] ?? [];

  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [models, setModels] = useState<ModelInfo[] | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const ref = useRef<HTMLDivElement>(null);
  const cacheKey = `${value.provider}:${kind}`;

  const load = async (force = false) => {
    if (!keyed) return;
    if (!force && cache.has(cacheKey)) {
      setModels(cache.get(cacheKey)!);
      return;
    }
    setLoading(true);
    setError(null);
    try {
      const list = await api.listModels(value.provider, kind);
      cache.set(cacheKey, list);
      setModels(list);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    setModels(cache.get(cacheKey) ?? null);
    setError(null);
  }, [cacheKey]);

  useEffect(() => {
    if (!open) return;
    load();
    const close = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    window.addEventListener("mousedown", close);
    return () => window.removeEventListener("mousedown", close);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    const sug = suggestions.filter((s) => !q || s.id.toLowerCase().includes(q) || s.label.toLowerCase().includes(q));
    const sugIds = new Set(suggestions.map((s) => s.id));
    const rest = (models ?? []).filter(
      (m) => !sugIds.has(m.id) && (!q || m.id.toLowerCase().includes(q) || m.name.toLowerCase().includes(q)),
    );
    return { sug, rest: rest.slice(0, 200), restTotal: rest.length };
  }, [query, suggestions, models]);

  const currentLabel = suggestions.find((s) => s.id === value.model)?.label;

  const pick = (id: string) => {
    onChange({ ...value, model: id });
    setOpen(false);
    setQuery("");
  };

  return (
    <div className="space-y-2">
      <div className="grid grid-cols-[minmax(170px,0.8fr)_2fr] gap-2.5">
        <Select<ProviderId>
          value={value.provider}
          onChange={(p) => {
            const first = ((kind === "stt" ? STT_SUGGESTIONS : LLM_SUGGESTIONS)[p] ?? [])[0];
            onChange({ provider: p, model: first?.id ?? "" });
          }}
          options={usable.map((p) => ({
            value: p.id,
            label: p.name,
            description: hasKey(settings, providers, p.id) ? undefined : "No API key yet",
          }))}
        />
        <div ref={ref} className="relative">
          <button
            type="button"
            className={cx(inputClass, "flex items-center justify-between gap-2 text-left", open && "border-brand-400/60")}
            onClick={() => setOpen((o) => !o)}
          >
            <span className="min-w-0 truncate">
              {value.model ? (
                <>
                  {currentLabel && <span className="text-white">{currentLabel}</span>}
                  <span className={cx("font-mono text-[12.5px]", currentLabel ? "ml-2 text-white/35" : "text-white")}>{value.model}</span>
                </>
              ) : (
                <span className="text-white/30">Choose a model…</span>
              )}
            </span>
            <ChevronDown className={cx("size-4 shrink-0 text-white/40 transition", open && "rotate-180")} />
          </button>
          {open && (
            <div className="absolute z-50 mt-1.5 w-full overflow-hidden rounded-xl border border-white/10 bg-ink-800/95 shadow-2xl shadow-black/60 backdrop-blur-xl animate-fade-in">
              <div className="flex items-center gap-2 border-b border-white/[0.06] px-3">
                <Search className="size-4 text-white/35" />
                <input
                  autoFocus
                  value={query}
                  onChange={(e) => setQuery(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" && query.trim()) pick(query.trim());
                    if (e.key === "Escape") setOpen(false);
                  }}
                  placeholder="Search models or type any model ID…"
                  className="h-10 flex-1 bg-transparent text-[13.5px] text-white outline-none placeholder:text-white/30"
                  spellCheck={false}
                />
                {loading ? (
                  <Loader2 className="size-4 animate-spin text-white/40" />
                ) : (
                  <button title="Refresh" onClick={() => load(true)} className="text-white/35 hover:text-white">
                    <RefreshCw className="size-3.5" />
                  </button>
                )}
              </div>
              <div className="max-h-80 overflow-y-auto p-1 scroll-area">
                {filtered.sug.length > 0 && <div className="px-3 pb-1 pt-2 text-[11px] font-semibold uppercase tracking-wider text-white/30">Recommended</div>}
                {filtered.sug.map((s) => (
                  <button
                    key={s.id}
                    onClick={() => pick(s.id)}
                    className={cx(
                      "flex w-full items-center gap-2 rounded-lg px-3 py-2 text-left transition",
                      s.id === value.model ? "bg-brand-500/15" : "hover:bg-white/[0.06]",
                    )}
                  >
                    <div className="min-w-0 flex-1">
                      <div className="text-[13.5px] text-white/90">{s.label}</div>
                      <div className="truncate font-mono text-[11.5px] text-white/35">{s.id}</div>
                    </div>
                    <div className="flex gap-1">
                      {s.tags?.map((t) => (
                        <Badge key={t} className={TAG_STYLE[t]}>
                          {t}
                        </Badge>
                      ))}
                    </div>
                  </button>
                ))}
                {filtered.rest.length > 0 && (
                  <div className="px-3 pb-1 pt-3 text-[11px] font-semibold uppercase tracking-wider text-white/30">
                    All models · {filtered.restTotal}
                  </div>
                )}
                {filtered.rest.map((m) => (
                  <button
                    key={m.id}
                    onClick={() => pick(m.id)}
                    className={cx(
                      "flex w-full items-center rounded-lg px-3 py-1.5 text-left transition",
                      m.id === value.model ? "bg-brand-500/15" : "hover:bg-white/[0.06]",
                    )}
                  >
                    <span className="min-w-0 flex-1 truncate font-mono text-[12.5px] text-white/75">{m.id}</span>
                    {m.name !== m.id && <span className="ml-3 max-w-[45%] truncate text-[12px] text-white/35">{m.name}</span>}
                  </button>
                ))}
                {error && <div className="px-3 py-3 text-[12.5px] text-rose-300/90">{error}</div>}
                {!loading && !error && filtered.sug.length === 0 && filtered.rest.length === 0 && (
                  <div className="px-3 py-3 text-[12.5px] text-white/40">
                    {query ? (
                      <>
                        Press <kbd className="kbd px-1.5 text-[11px]">Enter</kbd> to use “{query}”
                      </>
                    ) : keyed ? (
                      "No models found"
                    ) : (
                      "Add an API key to browse models"
                    )}
                  </div>
                )}
              </div>
            </div>
          )}
        </div>
      </div>
      {!keyed && (
        <div className="flex items-center gap-2 text-[12.5px] text-amber-300/90">
          <AlertTriangle className="size-3.5" />
          No API key for {providers.find((p) => p.id === value.provider)?.name}.
          {onNavigate && (
            <button className="font-medium underline underline-offset-2 hover:text-amber-200" onClick={() => onNavigate("providers")}>
              Add one
            </button>
          )}
        </div>
      )}
    </div>
  );
}
