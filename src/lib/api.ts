import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useEffect, useRef } from "react";

// ---------------------------------------------------------------------------------------------
// Types (mirror src-tauri/src/settings.rs and friends)
// ---------------------------------------------------------------------------------------------

export type ProviderId = "openrouter" | "openai" | "groq" | "mistral" | "gemini" | "anthropic" | "custom";

export interface ProviderConfig {
  api_key: string;
  base_url: string;
}

export interface ModelRef {
  provider: ProviderId;
  model: string;
}

export interface Replacement {
  from: string;
  to: string;
}

export interface QuickAction {
  id: string;
  name: string;
  prompt: string;
  icon: string;
}

export type HotkeyMode = "toggle" | "hold" | "hybrid";
export type HotkeyBackend = "auto" | "evdev" | "portal" | "native" | "disabled";
export type SelectionMethod = "copy" | "primary" | "none";
export type OutputMethod = "paste" | "type" | "clipboard";
export type PasteShortcut = "ctrl_v" | "ctrl_shift_v" | "shift_insert";
export type OverlayPosition = "bottom" | "top";

export interface Settings {
  onboarded: boolean;
  providers: Record<ProviderId, ProviderConfig>;
  dictation: {
    stt: ModelRef;
    language: string;
    vocabulary: string;
    polish: boolean;
    polish_model: ModelRef;
    polish_prompt: string;
    replacements: Replacement[];
    trailing_space: boolean;
  };
  command: {
    enabled: boolean;
    llm: ModelRef;
    selection_method: SelectionMethod;
    prompt_selection: string;
    prompt_generate: string;
    temperature: number;
    quick_actions: QuickAction[];
  };
  hotkeys: {
    backend: HotkeyBackend;
    dictate: string;
    dictate_mode: HotkeyMode;
    command: string;
    command_mode: HotkeyMode;
    hold_threshold_ms: number;
    escape_cancels: boolean;
  };
  audio: {
    input_device: string;
    sounds: boolean;
    sound_volume: number;
    max_seconds: number;
    trim_silence: boolean;
  };
  output: {
    method: OutputMethod;
    paste_shortcut: PasteShortcut;
    restore_clipboard: boolean;
  };
  general: {
    show_overlay: boolean;
    overlay_position: OverlayPosition;
    history_enabled: boolean;
    history_limit: number;
    start_hidden: boolean;
    theme: string;
  };
}

export interface ProviderInfo {
  id: ProviderId;
  name: string;
  base_url: string;
  stt: "open_router_json" | "multipart" | "none";
  llm: boolean;
  key_url: string;
  key_hint: string;
  needs_key: boolean;
}

export interface ModelInfo {
  id: string;
  name: string;
}

export type Mode = "dictate" | "command" | "pending" | "action";
export type Phase = "idle" | "recording" | "processing" | "done" | "error" | "cancelled";

export interface Status {
  phase: Phase;
  mode: Mode;
  message: string | null;
  preview: string | null;
  session: number;
  selection_chars: number | null;
  step: "transcribing" | "thinking" | "polishing" | "inserting" | null;
}

export interface HistoryEntry {
  id: number;
  ts: number;
  kind: "dictation" | "command" | "action";
  text: string;
  transcript: string;
  selection: string | null;
  audio_secs: number;
  latency_ms: number;
  stt_model: string;
  llm_model: string | null;
  words: number;
  inserted: boolean;
}

export interface Stats {
  words: number;
  audio_secs: number;
  dictations: number;
  commands: number;
  latency_ms_total: number;
  first_use: number;
}

export interface SystemStatus {
  version: string;
  environment: {
    os: "linux" | "macos" | "windows";
    session_type: string;
    desktop: string;
    input_access: boolean;
    uinput_access: boolean;
    relogin_required: boolean;
    layer_shell: boolean;
  };
  hotkeys: {
    backend: string;
    ok: boolean;
    message: string;
    portal_triggers: [string, string][];
  };
  injection_ok: boolean;
  injection_error: string | null;
}

// ---------------------------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------------------------

export const api = {
  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) => invoke<void>("save_settings", { settings }),
  resetSettings: () => invoke<Settings>("reset_settings"),
  defaultSettings: () => invoke<Settings>("default_settings"),
  providers: () => invoke<ProviderInfo[]>("providers_info"),
  listModels: (provider: ProviderId, kind: "stt" | "llm") => invoke<ModelInfo[]>("list_models", { provider, kind }),
  testProvider: (provider: ProviderId, apiKey: string, baseUrl: string) =>
    invoke<number>("test_provider", { provider, apiKey, baseUrl }),
  inputDevices: () => invoke<string[]>("input_devices"),
  systemStatus: () => invoke<SystemStatus>("system_status"),
  reapplyHotkeys: () => invoke<SystemStatus["hotkeys"]>("reapply_hotkeys"),
  history: () => invoke<HistoryEntry[]>("history_list"),
  deleteHistory: (id: number) => invoke<void>("history_delete", { id }),
  clearHistory: () => invoke<void>("history_clear"),
  stats: () => invoke<Stats>("stats"),
  lastStatus: () => invoke<Status | null>("last_status"),
  trigger: (action: "dictate" | "command" | "stop" | "cancel" | "action", arg?: string) =>
    invoke<void>("trigger", { action, arg }),
  copy: (text: string) => invoke<void>("copy_text", { text }),
  playSound: (name: string) => invoke<void>("play_sound", { name }),
  micTest: (start: boolean, device?: string) => invoke<void>("mic_test", { start, device }),
  fixPermissions: (inputGroup: boolean) => invoke<string>("fix_permissions", { inputGroup }),
  pauseHotkeys: (paused: boolean) => invoke<void>("pause_hotkeys", { paused }),
};

/** Subscribes to a backend event for the lifetime of the component. */
export function useEvent<T>(name: string, handler: (payload: T) => void) {
  const ref = useRef(handler);
  ref.current = handler;
  useEffect(() => {
    let un: UnlistenFn | undefined;
    let cancelled = false;
    listen<T>(name, (e) => ref.current(e.payload)).then((u) => {
      if (cancelled) u();
      else un = u;
    });
    return () => {
      cancelled = true;
      un?.();
    };
  }, [name]);
}

export function errorText(e: unknown): string {
  if (typeof e === "string") return e;
  if (e && typeof e === "object" && "message" in e) return String((e as { message: unknown }).message);
  return String(e);
}
