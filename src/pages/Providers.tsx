import { openUrl } from "@tauri-apps/plugin-opener";
import { CheckCircle2, ExternalLink, KeyRound, Server, XCircle } from "lucide-react";
import { useState } from "react";
import { Badge, Button, Field, Input, PageHeader, SecretInput, cx } from "../components/ui";
import { api, errorText, type ProviderId, type ProviderInfo } from "../lib/api";
import { useStore } from "../lib/store";

const BLURB: Record<ProviderId, string> = {
  openrouter: "One key for hundreds of models — Whisper, Parakeet, Voxtral, Claude, Gemini, GPT and more.",
  openai: "GPT-4o Transcribe and Whisper for speech, GPT models for commands.",
  groq: "Blazing-fast Whisper Large v3 Turbo and open models on LPUs.",
  mistral: "Voxtral speech recognition and Mistral language models.",
  gemini: "Google's Gemini models for commands and polishing.",
  anthropic: "Claude models for commands and polishing.",
  custom: "Any OpenAI-compatible server: speaches, LocalAI, whisper.cpp server, Ollama, LM Studio, vLLM…",
};

function ProviderCard({ info }: { info: ProviderInfo }) {
  const { settings, update } = useStore();
  const cfg = settings.providers[info.id];
  const [state, setState] = useState<{ ok: boolean; text: string } | null>(null);
  const [testing, setTesting] = useState(false);
  const configured = info.needs_key ? cfg.api_key.trim().length > 0 : cfg.base_url.trim().length > 0;

  const test = async () => {
    setTesting(true);
    setState(null);
    try {
      const n = await api.testProvider(info.id, cfg.api_key, cfg.base_url);
      setState({ ok: true, text: `Connected · ${n} models available` });
    } catch (e) {
      setState({ ok: false, text: errorText(e) });
    } finally {
      setTesting(false);
    }
  };

  return (
    <section className={cx("card p-5 transition", configured && "border-brand-400/20")}>
      <div className="mb-4 flex items-start justify-between gap-4">
        <div className="flex items-start gap-3">
          <div
            className={cx(
              "flex size-10 shrink-0 items-center justify-center rounded-xl",
              configured ? "brand-gradient text-white" : "bg-white/[0.06] text-white/50",
            )}
          >
            {info.id === "custom" ? <Server className="size-4.5" /> : <KeyRound className="size-4.5" />}
          </div>
          <div>
            <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
              <h3 className="text-[15px] font-semibold text-white">{info.name}</h3>
              {info.id === "openrouter" && <Badge className="bg-brand-500/15 text-brand-300">Recommended</Badge>}
              {info.stt !== "none" && <Badge>Speech</Badge>}
              {info.llm && <Badge>LLM</Badge>}
            </div>
            <p className="mt-1 text-[12.5px] leading-relaxed text-white/45">{BLURB[info.id]}</p>
          </div>
        </div>
      </div>
      <div className={cx("grid gap-3", info.id === "custom" ? "grid-cols-2" : "grid-cols-1")}>
        {info.id === "custom" && (
          <Field label="Base URL">
            <Input
              value={cfg.base_url}
              placeholder={info.base_url}
              onChange={(e) => update((s) => void (s.providers[info.id].base_url = e.target.value))}
            />
          </Field>
        )}
        <Field label={info.needs_key ? "API key" : "API key (optional)"}>
          <SecretInput
            value={cfg.api_key}
            placeholder={info.key_hint}
            onChange={(v) => {
              setState(null);
              update((s) => void (s.providers[info.id].api_key = v));
            }}
          />
        </Field>
      </div>
      <div className="mt-3 flex items-center justify-between gap-3">
        <div className="min-h-5 text-[12.5px]">
          {state && (
            <span className={cx("flex items-start gap-1.5", state.ok ? "text-emerald-300" : "text-rose-300")}>
              {state.ok ? <CheckCircle2 className="mt-0.5 size-3.5 shrink-0" /> : <XCircle className="mt-0.5 size-3.5 shrink-0" />}
              <span className="line-clamp-2">{state.text}</span>
            </span>
          )}
        </div>
        <div className="flex shrink-0 items-center gap-1">
          {info.key_url && (
            <Button size="sm" variant="ghost" icon={<ExternalLink className="size-3.5" />} onClick={() => openUrl(info.key_url)}>
              Get a key
            </Button>
          )}
          <Button size="sm" variant="secondary" loading={testing} disabled={!configured} onClick={test}>
            Test
          </Button>
        </div>
      </div>
    </section>
  );
}

export function Providers() {
  const { providers } = useStore();
  return (
    <div>
      <PageHeader
        title="Providers"
        description="Bring your own keys. They're stored locally on this computer and only sent to the provider you choose."
      />
      <div className="grid grid-cols-2 gap-4">
        {providers.map((p) => (
          <ProviderCard key={p.id} info={p} />
        ))}
      </div>
    </div>
  );
}
