import { AudioLines, BookA, Plus, Replace, RotateCcw, Sparkles, Trash2 } from "lucide-react";
import { useState } from "react";
import { ModelPicker } from "../components/ModelPicker";
import { Button, Field, Input, PageHeader, Row, Section, Select, TextArea, Toggle } from "../components/ui";
import { api } from "../lib/api";
import { LANGUAGES } from "../lib/catalog";
import { useStore } from "../lib/store";

const DEFAULT_POLISH_PROMPT_HINT = "Cleans punctuation and filler words without changing your meaning.";

export function Dictation({ navigate }: { navigate: (p: string) => void }) {
  const { settings, update } = useStore();
  const d = settings.dictation;
  const [showPrompt, setShowPrompt] = useState(false);

  return (
    <div>
      <PageHeader
        title="Dictation"
        description="Speak naturally and OpenDictate types it into whatever app you're using. Choose the speech model, language and how the text is cleaned up."
      />

      <Section icon={<AudioLines className="size-4.5" />} title="Speech recognition" description="The model that turns your voice into text.">
        <div className="space-y-5">
          <ModelPicker kind="stt" value={d.stt} onChange={(v) => update((s) => void (s.dictation.stt = v))} onNavigate={navigate} />
          <div className="grid grid-cols-2 gap-4">
            <Field label="Language" hint="Auto-detect works well; pinning a language is a bit faster and more accurate.">
              <Select
                value={d.language || "auto"}
                onChange={(v) => update((s) => void (s.dictation.language = v))}
                options={LANGUAGES.map((l) => ({ value: l.code, label: l.name, description: l.code === "auto" ? undefined : l.code }))}
              />
            </Field>
            <Field label="Vocabulary" hint="Names, jargon and spellings to help the recognizer (OpenAI-compatible providers).">
              <Input
                value={d.vocabulary}
                placeholder="e.g. OpenDictate, Kubernetes, Gonzalez"
                onChange={(e) => update((s) => void (s.dictation.vocabulary = e.target.value))}
              />
            </Field>
          </div>
        </div>
      </Section>

      <Section
        icon={<Sparkles className="size-4.5" />}
        title="AI polish"
        description="Run each dictation through a language model to fix punctuation, remove “um”s and false starts, and format lists."
        actions={<Toggle checked={d.polish} onChange={(v) => update((s) => void (s.dictation.polish = v))} />}
      >
        <div className={d.polish ? "space-y-4" : "pointer-events-none space-y-4 opacity-40"}>
          <ModelPicker
            kind="llm"
            value={d.polish_model}
            onChange={(v) => update((s) => void (s.dictation.polish_model = v))}
            onNavigate={navigate}
          />
          <div className="flex items-center justify-between">
            <div className="text-[12.5px] text-white/45">{DEFAULT_POLISH_PROMPT_HINT}</div>
            <Button size="sm" variant="ghost" onClick={() => setShowPrompt((v) => !v)}>
              {showPrompt ? "Hide prompt" : "Edit prompt"}
            </Button>
          </div>
          {showPrompt && (
            <TextArea rows={6} value={d.polish_prompt} onChange={(e) => update((s) => void (s.dictation.polish_prompt = e.target.value))} />
          )}
        </div>
      </Section>

      <Section
        icon={<Replace className="size-4.5" />}
        title="Replacements"
        description="Fix words the model keeps getting wrong, or expand shortcuts — e.g. “my email” → you@example.com."
        actions={
          <Button
            size="sm"
            variant="secondary"
            icon={<Plus className="size-3.5" />}
            onClick={() => update((s) => void s.dictation.replacements.push({ from: "", to: "" }))}
          >
            Add
          </Button>
        }
      >
        {d.replacements.length === 0 ? (
          <div className="rounded-xl border border-dashed border-white/10 py-6 text-center text-[13px] text-white/35">No replacements yet.</div>
        ) : (
          <div className="space-y-2">
            {d.replacements.map((r, i) => (
              <div key={i} className="grid grid-cols-[1fr_auto_1fr_auto] items-center gap-2">
                <Input value={r.from} placeholder="When I say…" onChange={(e) => update((s) => void (s.dictation.replacements[i].from = e.target.value))} />
                <span className="text-white/30">→</span>
                <Input value={r.to} placeholder="…type this" onChange={(e) => update((s) => void (s.dictation.replacements[i].to = e.target.value))} />
                <button
                  className="flex size-9 items-center justify-center rounded-lg text-white/35 hover:bg-white/[0.06] hover:text-rose-300"
                  onClick={() => update((s) => void s.dictation.replacements.splice(i, 1))}
                  title="Remove"
                >
                  <Trash2 className="size-4" />
                </button>
              </div>
            ))}
          </div>
        )}
      </Section>

      <Section icon={<BookA className="size-4.5" />} title="Formatting">
        <Row label="Add a trailing space" description="So the next dictation continues the sentence naturally.">
          <Toggle checked={d.trailing_space} onChange={(v) => update((s) => void (s.dictation.trailing_space = v))} />
        </Row>
        <Row label="Trim silence" description="Cut silence at the start and end before uploading — faster and cheaper.">
          <Toggle checked={settings.audio.trim_silence} onChange={(v) => update((s) => void (s.audio.trim_silence = v))} />
        </Row>
        <Row label="Reset polish prompt" description="Restore the built-in cleanup instructions.">
          <Button
            size="sm"
            variant="ghost"
            icon={<RotateCcw className="size-3.5" />}
            onClick={async () => {
              const defaults = await api.defaultSettings();
              update((s) => void (s.dictation.polish_prompt = defaults.dictation.polish_prompt));
            }}
          >
            Reset
          </Button>
        </Row>
      </Section>
    </div>
  );
}
