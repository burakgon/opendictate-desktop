import {
  Brain,
  Briefcase,
  Languages,
  Lightbulb,
  List,
  MousePointerClick,
  Play,
  Plus,
  RotateCcw,
  Scissors,
  Sparkles,
  SpellCheck,
  Trash2,
  Wand2,
  type LucideIcon,
} from "lucide-react";
import { useState } from "react";
import { ModelPicker } from "../components/ModelPicker";
import { Button, Field, Input, Keycaps, PageHeader, Row, Section, Segmented, Slider, TextArea, Toggle } from "../components/ui";
import { api, type QuickAction, type Reasoning, type SelectionMethod } from "../lib/api";
import { sameAccelerator } from "../lib/keys";
import { useStore } from "../lib/store";

export const ACTION_ICONS: Record<string, LucideIcon> = {
  sparkles: Sparkles,
  "spell-check": SpellCheck,
  briefcase: Briefcase,
  scissors: Scissors,
  languages: Languages,
  list: List,
  lightbulb: Lightbulb,
  wand: Wand2,
  brain: Brain,
};

const EXAMPLES = [
  { sel: true, say: "Make this sound more friendly" },
  { sel: true, say: "Translate to Spanish" },
  { sel: true, say: "Turn this into a bulleted summary" },
  { sel: true, say: "Fix the grammar but keep my tone" },
  { sel: false, say: "Write a polite reply saying I'll be ten minutes late" },
  { sel: false, say: "What's the capital of Australia?" },
];

export function Commands({ navigate }: { navigate: (p: string) => void }) {
  const { settings, update, system } = useStore();
  const c = settings.command;
  const os = system?.environment.os;
  const shared = sameAccelerator(settings.hotkeys.dictate, settings.hotkeys.command);
  const [prompts, setPrompts] = useState(false);

  const setAction = (i: number, fn: (a: QuickAction) => void) => update((s) => fn(s.command.quick_actions[i]));

  return (
    <div>
      <PageHeader
        title="Voice commands"
        description="Select text anywhere, hold your command key and say what to do with it. With nothing selected, OpenDictate writes something new at your cursor."
        actions={<Toggle checked={c.enabled} onChange={(v) => update((s) => void (s.command.enabled = v))} />}
      />

      <div className={c.enabled ? "" : "pointer-events-none opacity-40"}>
        <Section icon={<Wand2 className="size-4.5" />} title="How it works">
          <div className="grid grid-cols-[auto_1fr] items-center gap-x-5 gap-y-3 text-[13.5px] text-white/65">
            <Keycaps accel={settings.hotkeys.command} os={os} />
            <div>
              {shared ? (
                <>
                  <b className="font-medium text-white/90">Hold</b> the key, speak, release — or <b className="font-medium text-white/90">double-tap</b> it, speak, and tap once more.
                </>
              ) : (
                <>Press the command shortcut, speak, then release (or press again).</>
              )}
            </div>
          </div>
          <div className="mt-5 grid grid-cols-2 gap-2.5">
            {EXAMPLES.map((e) => (
              <div key={e.say} className="flex items-start gap-3 rounded-xl border border-white/[0.06] bg-white/[0.02] px-3.5 py-3">
                <div
                  className={`mt-0.5 rounded-md px-1.5 py-0.5 text-[10.5px] font-semibold uppercase tracking-wide ${e.sel ? "bg-brand-500/15 text-brand-300" : "bg-accent-500/15 text-accent-400"}`}
                >
                  {e.sel ? "selection" : "cursor"}
                </div>
                <div className="text-[13.5px] text-white/80">“{e.say}”</div>
              </div>
            ))}
          </div>
        </Section>

        <Section icon={<Brain className="size-4.5" />} title="Language model" description="Used for commands and quick actions. Fast models feel best.">
          <ModelPicker kind="llm" value={c.llm} onChange={(v) => update((s) => void (s.command.llm = v))} onNavigate={navigate} />
          <div className="mt-5">
            <Row
              label="Thinking"
              description={
                c.reasoning === "off"
                  ? "Off: the fastest answers. Models that must think fall back to the minimum automatically."
                  : c.reasoning === "low"
                    ? "A little reasoning: slower, sometimes better at tricky requests."
                    : "Use the model's own default (can add seconds)."
              }
            >
              <Segmented<Reasoning>
                value={c.reasoning}
                onChange={(v) => update((s) => void (s.command.reasoning = v))}
                options={[
                  { value: "off", label: "Off" },
                  { value: "low", label: "Low" },
                  { value: "default", label: "Model default" },
                ]}
              />
            </Row>
            <Row
              label="Command word"
              description="Start a normal dictation with this word to run it as a command instead — e.g. “Jarvis, make this formal”. Leave empty to turn off."
            >
              <Input
                className="w-48"
                value={c.trigger_word}
                placeholder="e.g. Jarvis"
                onChange={(e) => update((s) => void (s.command.trigger_word = e.target.value))}
              />
            </Row>
            <Row label="Creativity" description="Lower is more literal, higher is more inventive.">
              <Slider value={c.temperature} min={0} max={1.2} step={0.1} onChange={(v) => update((s) => void (s.command.temperature = v))} />
              <span className="w-8 text-right font-mono text-[12.5px] text-white/50">{c.temperature.toFixed(1)}</span>
            </Row>
            <Row
              label="Read the selection by"
              description={
                c.selection_method === "copy"
                  ? "Briefly pressing the copy shortcut. Most accurate; your clipboard is restored."
                  : c.selection_method === "primary"
                    ? "Reading the Linux primary selection. No key presses, but it may hold an old selection."
                    : "Never reading it. Commands always write new text."
              }
            >
              <Segmented<SelectionMethod>
                value={c.selection_method}
                onChange={(v) => update((s) => void (s.command.selection_method = v))}
                options={[
                  { value: "copy", label: "Copy" },
                  ...(os === "linux" ? [{ value: "primary" as SelectionMethod, label: "Primary" }] : []),
                  { value: "none", label: "Off" },
                ]}
              />
            </Row>
          </div>
        </Section>

        <Section
          icon={<MousePointerClick className="size-4.5" />}
          title="Quick actions"
          description="One-click prompts for selected text, available from the tray menu, here, or `opendictate action <id>`."
          actions={
            <Button
              size="sm"
              variant="secondary"
              icon={<Plus className="size-3.5" />}
              onClick={() =>
                update((s) =>
                  void s.command.quick_actions.push({ id: `custom-${Date.now().toString(36)}`, name: "New action", prompt: "", icon: "sparkles" }),
                )
              }
            >
              Add
            </Button>
          }
        >
          <div className="space-y-2.5">
            {c.quick_actions.map((a, i) => {
              const Icon = ACTION_ICONS[a.icon] ?? Sparkles;
              return (
                <div key={a.id} className="group rounded-xl border border-white/[0.06] bg-white/[0.02] p-3.5">
                  <div className="flex items-center gap-3">
                    <div className="flex size-9 shrink-0 items-center justify-center rounded-lg bg-brand-500/12 text-brand-300">
                      <Icon className="size-4" />
                    </div>
                    <Input className="h-9 max-w-56 font-medium" value={a.name} onChange={(e) => setAction(i, (x) => void (x.name = e.target.value))} />
                    <Input className="h-9 flex-1" value={a.prompt} placeholder="What should happen to the selected text?" onChange={(e) => setAction(i, (x) => void (x.prompt = e.target.value))} />
                    <span className="font-mono text-[11px] text-white/25" title="ID for the CLI">
                      {a.id}
                    </span>
                    <button
                      className="flex size-8 items-center justify-center rounded-lg text-white/40 hover:bg-white/[0.06] hover:text-white"
                      title="Run on the current selection (switch to the target app within 2 seconds)"
                      onClick={() => setTimeout(() => api.trigger("action", a.id), 2000)}
                    >
                      <Play className="size-3.5" />
                    </button>
                    <button
                      className="flex size-8 items-center justify-center rounded-lg text-white/40 hover:bg-white/[0.06] hover:text-rose-300"
                      title="Remove"
                      onClick={() => update((s) => void s.command.quick_actions.splice(i, 1))}
                    >
                      <Trash2 className="size-3.5" />
                    </button>
                  </div>
                </div>
              );
            })}
          </div>
        </Section>

        <Section
          icon={<Sparkles className="size-4.5" />}
          title="System prompts"
          description="Advanced: the instructions given to the model."
          actions={
            <Button size="sm" variant="ghost" onClick={() => setPrompts((p) => !p)}>
              {prompts ? "Hide" : "Edit"}
            </Button>
          }
        >
          {prompts && (
            <div className="space-y-4">
              <Field label="When text is selected">
                <TextArea rows={6} value={c.prompt_selection} onChange={(e) => update((s) => void (s.command.prompt_selection = e.target.value))} />
              </Field>
              <Field label="When nothing is selected">
                <TextArea rows={5} value={c.prompt_generate} onChange={(e) => update((s) => void (s.command.prompt_generate = e.target.value))} />
              </Field>
              <Button
                size="sm"
                variant="ghost"
                icon={<RotateCcw className="size-3.5" />}
                onClick={async () => {
                  const d = await api.defaultSettings();
                  update((s) => {
                    s.command.prompt_selection = d.command.prompt_selection;
                    s.command.prompt_generate = d.command.prompt_generate;
                  });
                }}
              >
                Restore defaults
              </Button>
            </div>
          )}
        </Section>
      </div>
    </div>
  );
}
