import { openUrl } from "@tauri-apps/plugin-opener";
import { ArrowLeft, ArrowRight, Check, CheckCircle2, ExternalLink, Mic, PartyPopper, ShieldCheck, XCircle } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { Button, Keycaps, SecretInput, TextArea, cx, useToast } from "../components/ui";
import { LevelBars } from "../components/Visuals";
import { api, errorText, useEvent, type ProviderId } from "../lib/api";
import { LLM_SUGGESTIONS, STT_SUGGESTIONS } from "../lib/catalog";
import { sameAccelerator } from "../lib/keys";
import { useStore } from "../lib/store";

const PROVIDER_CHOICES: { id: ProviderId; title: string; text: string }[] = [
  { id: "openrouter", title: "OpenRouter", text: "One key, every model. Recommended." },
  { id: "openai", title: "OpenAI", text: "GPT Transcribe + GPT models." },
  { id: "groq", title: "Groq", text: "The fastest Whisper you can get." },
  { id: "mistral", title: "Mistral", text: "Voxtral speech + Mistral models." },
];

type Step = "welcome" | "provider" | "mic" | "permissions" | "try" | "done";

export function Onboarding({ onDone }: { onDone: () => void }) {
  const { settings, update, providers, system, refreshSystem } = useStore();
  const env = system?.environment;
  const needsPermissions = env?.os === "linux" && (!env.uinput_access || !env.input_access);
  const steps: Step[] = ["welcome", "provider", "mic", ...(needsPermissions ? (["permissions"] as Step[]) : []), "try", "done"];
  const [step, setStep] = useState<Step>("welcome");
  const idx = steps.indexOf(step);
  const next = () => setStep(steps[Math.min(idx + 1, steps.length - 1)]);
  const back = () => setStep(steps[Math.max(idx - 1, 0)]);
  const toast = useToast();

  // Provider step
  const [provider, setProvider] = useState<ProviderId>(settings.dictation.stt.provider);
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<{ ok: boolean; text: string } | null>(null);
  const key = settings.providers[provider]?.api_key ?? "";
  const info = providers.find((p) => p.id === provider);

  const testKey = async () => {
    setTesting(true);
    setTestResult(null);
    try {
      const n = await api.testProvider(provider, key, settings.providers[provider].base_url);
      setTestResult({ ok: true, text: `Connected — ${n} models available.` });
      let llm = LLM_SUGGESTIONS[provider]?.[0]?.id;
      if (!llm) {
        const list = await api.listModels(provider, "llm").catch(() => []);
        llm = list[0]?.id;
      }
      update((s) => {
        const stt = STT_SUGGESTIONS[provider]?.[0]?.id;
        if (stt) s.dictation.stt = { provider, model: stt };
        if (llm) {
          s.command.llm = { provider, model: llm };
          s.dictation.polish_model = { provider, model: llm };
        }
      });
    } catch (e) {
      setTestResult({ ok: false, text: errorText(e) });
    } finally {
      setTesting(false);
    }
  };

  // Mic step
  const [micOn, setMicOn] = useState(false);
  const [micError, setMicError] = useState<string | null>(null);
  useEffect(() => {
    if (step === "mic") {
      api
        .micTest(true)
        .then(() => setMicOn(true))
        .catch((e) => setMicError(errorText(e)));
      return () => {
        api.micTest(false).catch(() => {});
        setMicOn(false);
      };
    }
  }, [step]);

  // Try step
  const [tried, setTried] = useState(false);
  const [text, setText] = useState("");
  useEvent("od://history", () => step === "try" && setTried(true));

  const [fixing, setFixing] = useState(false);
  const shared = sameAccelerator(settings.hotkeys.dictate, settings.hotkeys.command);

  const finish = () => {
    update((s) => void (s.onboarded = true));
    onDone();
  };

  // Enter = the primary action of the current step.
  const primary = useRef<() => void>(() => {});
  primary.current = () => {
    if (step === "provider") {
      if (testResult?.ok) next();
      else if (key.trim() && !testing) testKey();
    } else if (step === "done") finish();
    else next();
  };
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Enter" || e.shiftKey || e.isComposing) return;
      const t = e.target as HTMLElement;
      if (t.tagName === "TEXTAREA" || t.tagName === "BUTTON") return;
      e.preventDefault();
      primary.current();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  return (
    <div className="app-frame relative flex h-full flex-col overflow-hidden rounded-[14px] border border-white/10">
      <div data-tauri-drag-region className="h-10 shrink-0" />
      <div className="pointer-events-none absolute -left-40 -top-40 size-[520px] rounded-full bg-brand-500/20 blur-3xl" />
      <div className="pointer-events-none absolute -bottom-40 -right-40 size-[520px] rounded-full bg-accent-500/15 blur-3xl" />

      <div className="relative flex flex-1 items-center justify-center px-10 pb-6">
        <div key={step} className="w-full max-w-xl animate-fade-in">
          {step === "welcome" && (
            <div className="text-center">
              <img src="/logo.svg" alt="" className="mx-auto mb-7 size-24 drop-shadow-[0_20px_40px_rgba(124,58,237,0.45)]" />
              <h1 className="text-[40px] font-semibold leading-tight tracking-tight text-white">
                Your voice, <span className="text-gradient">typed anywhere.</span>
              </h1>
              <p className="mx-auto mt-4 max-w-md text-[15px] leading-relaxed text-white/55">
                Tap a key to dictate into any app. Hold it to tell AI what to do with the text you've selected. Bring your own models.
              </p>
              <Button variant="primary" size="lg" className="mt-9" onClick={next}>
                Get started <ArrowRight className="size-4" />
              </Button>
            </div>
          )}

          {step === "provider" && (
            <div>
              <h2 className="text-[28px] font-semibold tracking-tight text-white">Connect a provider</h2>
              <p className="mt-2 text-[14px] text-white/50">OpenDictate uses your own API key. It stays on this computer.</p>
              <div className="mt-6 grid grid-cols-2 gap-2.5">
                {PROVIDER_CHOICES.map((p) => (
                  <button
                    key={p.id}
                    onClick={() => {
                      setProvider(p.id);
                      setTestResult(null);
                    }}
                    className={cx(
                      "rounded-xl border p-4 text-left transition",
                      provider === p.id ? "border-brand-400/60 bg-brand-500/10 ring-4 ring-brand-500/10" : "border-white/10 bg-white/[0.03] hover:bg-white/[0.06]",
                    )}
                  >
                    <div className="flex items-center justify-between text-[14px] font-semibold text-white">
                      {p.title}
                      {provider === p.id && <Check className="size-4 text-brand-300" />}
                    </div>
                    <div className="mt-1 text-[12.5px] text-white/45">{p.text}</div>
                  </button>
                ))}
              </div>
              <div className="mt-5">
                <div className="mb-1.5 flex items-center justify-between">
                  <span className="text-[12.5px] font-medium text-white/60">{info?.name} API key</span>
                  {info?.key_url && (
                    <button className="flex items-center gap-1 text-[12.5px] text-brand-300 hover:text-brand-200" onClick={() => openUrl(info.key_url)}>
                      Get a key <ExternalLink className="size-3" />
                    </button>
                  )}
                </div>
                <SecretInput
                  value={key}
                  placeholder={info?.key_hint}
                  onChange={(v) => {
                    setTestResult(null);
                    update((s) => void (s.providers[provider].api_key = v));
                  }}
                />
                {testResult && (
                  <div className={cx("mt-2 flex items-start gap-1.5 text-[13px]", testResult.ok ? "text-emerald-300" : "text-rose-300")}>
                    {testResult.ok ? <CheckCircle2 className="mt-0.5 size-4 shrink-0" /> : <XCircle className="mt-0.5 size-4 shrink-0" />}
                    {testResult.text}
                  </div>
                )}
              </div>
            </div>
          )}

          {step === "mic" && (
            <div className="text-center">
              <div className="mx-auto mb-6 flex size-16 items-center justify-center rounded-2xl bg-white/[0.06] text-brand-300">
                <Mic className="size-7" />
              </div>
              <h2 className="text-[28px] font-semibold tracking-tight text-white">Say something</h2>
              <p className="mt-2 text-[14px] text-white/50">The bars should move when you speak.</p>
              <div className="mt-8 flex justify-center">
                {micError ? (
                  <div className="max-w-md text-[13px] text-rose-300">{micError}</div>
                ) : (
                  <LevelBars event="od://mic-level" bars={40} className="h-20" active={micOn} />
                )}
              </div>
            </div>
          )}

          {step === "permissions" && (
            <div>
              <div className="mb-6 flex size-16 items-center justify-center rounded-2xl bg-white/[0.06] text-brand-300">
                <ShieldCheck className="size-7" />
              </div>
              <h2 className="text-[28px] font-semibold tracking-tight text-white">One-time Linux setup</h2>
              <p className="mt-2 text-[14px] leading-relaxed text-white/55">
                To paste text into other apps, OpenDictate creates a virtual keyboard. To use single keys like Right Ctrl as a shortcut it reads your keyboard. Both need a
                quick permission (you'll be asked for your password).
              </p>
              <div className="mt-5 space-y-2 rounded-xl border border-white/[0.07] bg-black/20 p-4 text-[13.5px]">
                <div className="flex items-center gap-2">
                  {env?.uinput_access ? <CheckCircle2 className="size-4 text-emerald-400" /> : <XCircle className="size-4 text-rose-400" />}
                  Paste into apps
                </div>
                <div className="flex items-center gap-2">
                  {env?.input_access ? <CheckCircle2 className="size-4 text-emerald-400" /> : <XCircle className="size-4 text-amber-400" />}
                  Single-key shortcuts like Right Ctrl
                </div>
              </div>
              <Button
                variant="primary"
                className="mt-5"
                loading={fixing}
                onClick={async () => {
                  setFixing(true);
                  try {
                    toast(await api.fixPermissions(true), "success");
                    await refreshSystem();
                  } catch (e) {
                    toast(errorText(e), "error");
                  } finally {
                    setFixing(false);
                  }
                }}
              >
                Grant permissions
              </Button>
            </div>
          )}

          {step === "try" && (
            <div>
              <h2 className="text-[28px] font-semibold tracking-tight text-white">Give it a try</h2>
              <div className="mt-4 space-y-2.5 text-[14px] text-white/60">
                <div className="flex items-center gap-3">
                  <Keycaps accel={settings.hotkeys.dictate} os={env?.os} />
                  <span>{shared ? "Tap, speak, tap again." : "Press, speak, press again."}</span>
                </div>
                {settings.command.enabled && (
                  <div className="flex items-center gap-3">
                    <Keycaps accel={settings.hotkeys.command} os={env?.os} />
                    <span>{shared ? "Hold or double-tap to speak a command." : "Voice command."}</span>
                  </div>
                )}
              </div>
              <TextArea autoFocus rows={5} className="mt-5" value={text} onChange={(e) => setText(e.target.value)} placeholder="Click here, then use your shortcut…" />
              <div className="mt-2 h-5 text-[13px]">
                {tried && (
                  <span className="flex items-center gap-1.5 text-emerald-300">
                    <CheckCircle2 className="size-4" /> It works! You can change shortcuts any time in Settings.
                  </span>
                )}
              </div>
            </div>
          )}

          {step === "done" && (
            <div className="text-center">
              <div className="mx-auto mb-6 flex size-16 items-center justify-center rounded-2xl brand-gradient text-white shadow-[0_20px_40px_-10px_rgba(124,58,237,0.7)]">
                <PartyPopper className="size-7" />
              </div>
              <h2 className="text-[32px] font-semibold tracking-tight text-white">You're all set</h2>
              <p className="mx-auto mt-3 max-w-md text-[14.5px] leading-relaxed text-white/55">
                OpenDictate lives in your system tray. Close this window any time — your shortcuts keep working.
              </p>
              <Button
                variant="primary"
                size="lg"
                className="mt-8"
                onClick={finish}
              >
                Start dictating
              </Button>
            </div>
          )}
        </div>
      </div>

      {step !== "welcome" && step !== "done" && (
        <div className="relative flex items-center justify-between px-10 pb-8">
          <Button variant="ghost" onClick={back} icon={<ArrowLeft className="size-4" />}>
            Back
          </Button>
          <div className="flex gap-1.5">
            {steps.map((s, i) => (
              <span key={s} className={cx("h-1.5 rounded-full transition-all", i === idx ? "w-6 bg-brand-400" : i < idx ? "w-1.5 bg-brand-400/60" : "w-1.5 bg-white/15")} />
            ))}
          </div>
          <div className="flex gap-2">
            {step === "provider" && !testResult?.ok ? (
              <>
                <Button variant="ghost" onClick={next}>
                  Skip
                </Button>
                <Button variant="primary" loading={testing} disabled={!key.trim()} onClick={testKey}>
                  Connect
                </Button>
              </>
            ) : (
              <Button variant="primary" onClick={next}>
                {step === "try" && !tried ? "Skip" : "Continue"} <ArrowRight className="size-4" />
              </Button>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
