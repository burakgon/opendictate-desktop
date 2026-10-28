import { disable as disableAutostart, enable as enableAutostart, isEnabled as autostartEnabled } from "@tauri-apps/plugin-autostart";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  CheckCircle2,
  ClipboardPaste,
  Keyboard,
  Mic,
  Monitor,
  RotateCcw,
  ShieldCheck,
  Terminal,
  Volume2,
  XCircle,
} from "lucide-react";
import { useEffect, useState } from "react";
import { GithubMark } from "../components/Brand";
import { HotkeyRecorder } from "../components/HotkeyRecorder";
import { Button, PageHeader, Row, Section, Segmented, Select, Slider, StatusDot, Toggle, cx, useToast } from "../components/ui";
import { LevelBars } from "../components/Visuals";
import {
  api,
  errorText,
  type HotkeyBackend,
  type HotkeyMode,
  type OutputMethod,
  type OverlayPosition,
  type PasteShortcut,
} from "../lib/api";
import { isModifier, sameAccelerator, splitAccelerator } from "../lib/keys";

const isModifierOnly = (accel: string) => {
  const parts = splitAccelerator(accel);
  return parts.length > 0 && parts.every(isModifier);
};
import { useStore } from "../lib/store";

const MODES: { value: HotkeyMode; label: string; title: string }[] = [
  { value: "hybrid", label: "Tap or hold", title: "Tap to start/stop, or hold to talk" },
  { value: "toggle", label: "Toggle", title: "Press to start, press again to stop" },
  { value: "hold", label: "Hold", title: "Records only while the key is held" },
];

function MicTest() {
  const { settings } = useStore();
  const [on, setOn] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => () => void api.micTest(false).catch(() => {}), []);
  return (
    <div className="flex items-center gap-3">
      {on && <LevelBars event="od://mic-level" bars={22} className="h-8" />}
      {error && <span className="max-w-60 truncate text-[12px] text-rose-300" title={error}>{error}</span>}
      <Button
        size="sm"
        variant={on ? "subtle" : "secondary"}
        onClick={async () => {
          setError(null);
          try {
            await api.micTest(!on, settings.audio.input_device);
            setOn(!on);
          } catch (e) {
            setError(errorText(e));
          }
        }}
      >
        {on ? "Stop test" : "Test"}
      </Button>
    </div>
  );
}

function Check({ ok, label, hint }: { ok: boolean; label: string; hint?: string }) {
  return (
    <div className="flex items-start gap-2.5">
      {ok ? <CheckCircle2 className="mt-0.5 size-4 shrink-0 text-emerald-400" /> : <XCircle className="mt-0.5 size-4 shrink-0 text-rose-400" />}
      <div>
        <div className="text-[13.5px] text-white/85">{label}</div>
        {hint && <div className="text-[12px] text-white/40">{hint}</div>}
      </div>
    </div>
  );
}

export function SettingsPage() {
  const { settings, update, system, refreshSystem } = useStore();
  const [devices, setDevices] = useState<{ id: string; label: string }[]>([]);
  const [autostart, setAutostart] = useState(false);
  const [fixing, setFixing] = useState(false);
  const toast = useToast();
  const env = system?.environment;
  const os = env?.os;
  const hk = settings.hotkeys;
  const shared = settings.command.enabled && sameAccelerator(hk.dictate, hk.command);

  useEffect(() => {
    api.inputDevices().then(setDevices);
    autostartEnabled().then(setAutostart).catch(() => {});
  }, []);

  const fix = async () => {
    setFixing(true);
    try {
      const msg = await api.fixPermissions(true);
      toast(msg, "success");
      await refreshSystem();
    } catch (e) {
      toast(errorText(e), "error");
    } finally {
      setFixing(false);
    }
  };

  const backendOptions: { value: HotkeyBackend; label: string; title: string }[] =
    os === "linux"
      ? [
          { value: "auto", label: "Auto", title: "Direct keyboard access when available, otherwise the desktop portal" },
          { value: "evdev", label: "Direct", title: "Read keyboards directly (input group). Supports single modifier keys." },
          { value: "portal", label: "Desktop portal", title: "Ask the desktop (KDE/GNOME/Hyprland) to register shortcuts" },
          { value: "disabled", label: "Off", title: "Use the tray or CLI only" },
        ]
      : [
          { value: "auto", label: "On", title: "Native global shortcuts" },
          { value: "disabled", label: "Off", title: "Use the tray only" },
        ];

  return (
    <div>
      <PageHeader title="Settings" description="Shortcuts, audio, output and everything else." />

      <Section
        icon={<Keyboard className="size-4.5" />}
        title="Shortcuts"
        description={
          shared
            ? "Dictation and commands share a key: tap it to dictate; hold it or double-tap it to speak a command."
            : "Tip: use the same key for both to get tap-to-dictate and hold (or double-tap) for a command."
        }
      >
        <Row label="Dictation" description={shared ? "Tap to start · tap again to stop" : MODES.find((m) => m.value === hk.dictate_mode)?.title}>
          {!shared && (
            <Segmented<HotkeyMode> size="sm" value={hk.dictate_mode} onChange={(v) => update((s) => void (s.hotkeys.dictate_mode = v))} options={MODES} />
          )}
          <HotkeyRecorder value={hk.dictate} os={os} allowMouse={os === "linux"} onChange={(v) => update((s) => void (s.hotkeys.dictate = v))} />
        </Row>
        <Row label="Voice command" description={shared ? "Hold and speak, then release — or double-tap, speak, tap" : MODES.find((m) => m.value === hk.command_mode)?.title}>
          {!shared && (
            <Segmented<HotkeyMode> size="sm" value={hk.command_mode} onChange={(v) => update((s) => void (s.hotkeys.command_mode = v))} options={MODES} />
          )}
          <HotkeyRecorder value={hk.command} os={os} allowMouse={os === "linux"} onChange={(v) => update((s) => void (s.hotkeys.command = v))} />
        </Row>
        <Row label="Hold threshold" description="How long a press counts as “hold” rather than “tap”.">
          <Slider value={hk.hold_threshold_ms} min={200} max={800} step={25} onChange={(v) => update((s) => void (s.hotkeys.hold_threshold_ms = v))} />
          <span className="w-14 text-right font-mono text-[12.5px] text-white/50">{hk.hold_threshold_ms} ms</span>
        </Row>
        <Row label="Esc cancels" description="Press Escape to throw away the current recording.">
          <Toggle checked={hk.escape_cancels} onChange={(v) => update((s) => void (s.hotkeys.escape_cancels = v))} />
        </Row>
        <Row
          label="Shortcut engine"
          description={
            <span className="flex items-center gap-2">
              <StatusDot ok={!!system?.hotkeys.ok} />
              <span>
                {system?.hotkeys.backend === "evdev" ? "Direct keyboard access" : system?.hotkeys.backend === "portal" ? "Desktop portal" : system?.hotkeys.backend}
                {" — "}
                {system?.hotkeys.message}
                {system?.hotkeys.portal_triggers?.length
                  ? ` (${system.hotkeys.portal_triggers.map(([id, t]) => `${id}: ${t || "unassigned"}`).join(", ")})`
                  : ""}
              </span>
            </span>
          }
        >
          <Segmented<HotkeyBackend> size="sm" value={hk.backend} onChange={(v) => update((s) => void (s.hotkeys.backend = v))} options={backendOptions} />
        </Row>
        {system?.hotkeys.backend === "portal" && (
          <div className="mt-4 flex items-center justify-between gap-4 rounded-xl border border-white/[0.06] bg-black/20 px-4 py-3 text-[12.5px] text-white/55">
            <span>With the desktop portal, your desktop owns the keys: confirm or change them in its shortcut settings (look for “OpenDictate”).</span>
            <Button size="sm" variant="secondary" onClick={() => api.openShortcutSettings().catch((e) => toast(errorText(e), "error"))}>
              Open shortcut settings
            </Button>
          </div>
        )}
        {system?.hotkeys.backend === "portal" && (isModifierOnly(hk.dictate) || isModifierOnly(hk.command)) && (
          <div className="mt-4 rounded-xl border border-amber-500/20 bg-amber-500/[0.07] px-4 py-3 text-[12.5px] text-amber-100/85">
            The desktop portal can't bind a single modifier key like Right Ctrl. Grant keyboard access in <b>Linux permissions</b> below, or
            choose a combination such as Ctrl+Alt+Space.
          </div>
        )}
        <div className="mt-4 flex items-start gap-3 rounded-xl border border-white/[0.06] bg-black/20 px-4 py-3 text-[12.5px] text-white/50">
          <Terminal className="mt-0.5 size-4 shrink-0 text-white/40" />
          <div>
            You can also bind these commands in your desktop's shortcut settings:{" "}
            <code className="rounded bg-white/[0.06] px-1.5 py-0.5 font-mono text-white/75">opendictate toggle</code>{" "}
            <code className="rounded bg-white/[0.06] px-1.5 py-0.5 font-mono text-white/75">opendictate command</code>{" "}
            <code className="rounded bg-white/[0.06] px-1.5 py-0.5 font-mono text-white/75">opendictate cancel</code>
          </div>
        </div>
      </Section>

      {os === "linux" && system && (
        <Section
          icon={<ShieldCheck className="size-4.5" />}
          title="Linux permissions"
          description="OpenDictate needs to see your shortcut key and to press Ctrl+V for you. Both are granted once."
          actions={
            (!env?.input_access || !env?.uinput_access) && (
              <Button size="sm" variant="primary" loading={fixing} onClick={fix}>
                Fix automatically
              </Button>
            )
          }
        >
          <div className="grid grid-cols-2 gap-4">
            <Check
              ok={!!env?.uinput_access && system.injection_ok}
              label="Paste into apps (/dev/uinput)"
              hint={env?.uinput_access ? "Virtual keyboard ready" : "Needed to insert text into other apps"}
            />
            <Check
              ok={!!env?.input_access}
              label="Read shortcut keys (/dev/input)"
              hint={env?.input_access ? "Single-key shortcuts like Right Ctrl work" : "Optional — without it the desktop portal is used"}
            />
            <Check
              ok={!!env?.overlay_available}
              label="Overlay above all windows"
              hint={env?.layer_shell ? "Using wlr-layer-shell" : env?.overlay_available ? "Using an always-on-top window" : "Not supported by this desktop"}
            />
            <Check ok={!!env?.session_type} label={`Session: ${env?.session_type || "unknown"} · ${env?.desktop || "unknown desktop"}`} />
          </div>
        </Section>
      )}

      <Section icon={<Mic className="size-4.5" />} title="Microphone">
        <Row label="Input device" description="“System default” follows your system's sound settings.">
          <Select
            className="w-72"
            value={settings.audio.input_device}
            onChange={(v) => update((s) => void (s.audio.input_device = v))}
            options={[
              { value: "", label: "System default" },
              ...devices.map((d) => ({ value: d.id, label: d.label })),
              ...(settings.audio.input_device && !devices.some((d) => d.id === settings.audio.input_device)
                ? [{ value: settings.audio.input_device, label: `${settings.audio.input_device} (not connected)` }]
                : []),
            ]}
          />
        </Row>
        <Row label="Level check" description="Speak and watch the bars move.">
          <MicTest />
        </Row>
        <Row label="Maximum length" description="Recording stops automatically after this long.">
          <Select
            className="w-40"
            value={String(settings.audio.max_seconds)}
            onChange={(v) => update((s) => void (s.audio.max_seconds = Number(v)))}
            options={[60, 180, 300, 600, 1200].map((n) => ({ value: String(n), label: n < 60 ? `${n}s` : `${n / 60} min` }))}
          />
        </Row>
      </Section>

      <Section icon={<Volume2 className="size-4.5" />} title="Sounds">
        <Row label="Play sounds" description="Short chimes when recording starts and stops.">
          <Toggle checked={settings.audio.sounds} onChange={(v) => update((s) => void (s.audio.sounds = v))} />
        </Row>
        <Row label="Volume">
          <Slider value={settings.audio.sound_volume} min={0.05} max={1} step={0.05} onChange={(v) => update((s) => void (s.audio.sound_volume = v))} />
          <Button size="sm" variant="ghost" onClick={() => api.playSound("start")}>
            Preview
          </Button>
        </Row>
      </Section>

      <Section icon={<ClipboardPaste className="size-4.5" />} title="Output">
        <Row
          label="Insert text by"
          description={
            settings.output.method === "paste"
              ? "Placing it on the clipboard and pressing the paste shortcut. Fast and works with every language."
              : settings.output.method === "type"
                ? "Typing it key by key (macOS/Windows; falls back to pasting on Linux)."
                : "Only copying it — you paste it yourself."
          }
        >
          <Segmented<OutputMethod>
            size="sm"
            value={settings.output.method}
            onChange={(v) => update((s) => void (s.output.method = v))}
            options={[
              { value: "paste", label: "Paste" },
              { value: "type", label: "Type" },
              { value: "clipboard", label: "Clipboard only" },
            ]}
          />
        </Row>
        <Row
          label="Paste shortcut"
          description={
            settings.output.paste_shortcut === "auto"
              ? "Auto uses Ctrl+V, and Ctrl+Shift+V (and Ctrl+Shift+C to read selections) when a terminal has focus."
              : "Terminals usually want Ctrl+Shift+V. Shift+Insert works almost everywhere on Linux."
          }
        >
          <Segmented<PasteShortcut>
            size="sm"
            value={settings.output.paste_shortcut}
            onChange={(v) => update((s) => void (s.output.paste_shortcut = v))}
            options={[
              { value: "auto", label: "Auto" },
              { value: "ctrl_v", label: os === "macos" ? "⌘V" : "Ctrl+V" },
              { value: "ctrl_shift_v", label: os === "macos" ? "⌘⇧V" : "Ctrl+Shift+V" },
              { value: "shift_insert", label: "Shift+Insert" },
            ]}
          />
        </Row>
        <Row label="Restore clipboard" description="Put back what was on your clipboard after pasting.">
          <Toggle checked={settings.output.restore_clipboard} onChange={(v) => update((s) => void (s.output.restore_clipboard = v))} />
        </Row>
      </Section>

      <Section icon={<Monitor className="size-4.5" />} title="Interface">
        <Row
          label="Show overlay"
          description={
            env && !env.overlay_available
              ? "Your desktop can't keep a floating pill above other windows without stealing focus (GNOME on Wayland), so OpenDictate uses sounds and the tray icon instead."
              : "A small pill with a live waveform while listening."
          }
        >
          <Toggle
            checked={settings.general.show_overlay && (env?.overlay_available ?? true)}
            disabled={env ? !env.overlay_available : false}
            onChange={(v) => update((s) => void (s.general.show_overlay = v))}
          />
        </Row>
        <Row label="Overlay position">
          <Segmented<OverlayPosition>
            size="sm"
            value={settings.general.overlay_position}
            onChange={(v) => update((s) => void (s.general.overlay_position = v))}
            options={[
              { value: "bottom", label: "Bottom" },
              { value: "top", label: "Top" },
            ]}
          />
        </Row>
        <Row label="Launch at login">
          <Toggle
            checked={autostart}
            onChange={async (v) => {
              try {
                if (v) await enableAutostart();
                else await disableAutostart();
                setAutostart(v);
              } catch (e) {
                toast(errorText(e), "error");
              }
            }}
          />
        </Row>
        <Row label="Start in the tray" description="Don't open this window when OpenDictate starts.">
          <Toggle checked={settings.general.start_hidden} onChange={(v) => update((s) => void (s.general.start_hidden = v))} />
        </Row>
        <Row label="Keep history" description="Stored only on this computer.">
          <Toggle checked={settings.general.history_enabled} onChange={(v) => update((s) => void (s.general.history_enabled = v))} />
        </Row>
      </Section>

      <Section title="About">
        <div className="flex items-center justify-between gap-4">
          <div className="flex items-center gap-4">
            <img src="/logo.svg" className="size-12" alt="" />
            <div>
              <div className="text-[15px] font-semibold text-white">OpenDictate {system?.version}</div>
              <div className="text-[12.5px] text-white/45">Free and open source under the MIT license.</div>
            </div>
          </div>
          <div className="flex gap-2">
            <Button size="sm" variant="secondary" icon={<GithubMark className="size-3.5" />} onClick={() => openUrl("https://github.com/burakgon/opendictate-desktop")}>
              GitHub
            </Button>
            <Button
              size="sm"
              variant="ghost"
              icon={<RotateCcw className="size-3.5" />}
              onClick={async () => {
                await api.resetSettings();
                toast("Settings reset (API keys kept)", "success");
              }}
            >
              Reset settings
            </Button>
          </div>
        </div>
      </Section>
      <div className={cx("h-4")} />
    </div>
  );
}
