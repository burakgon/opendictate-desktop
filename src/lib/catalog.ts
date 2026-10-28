import type { ProviderId } from "./api";

export interface Suggestion {
  id: string;
  label: string;
  tags?: string[];
}

/** Hand-picked defaults shown before (and above) the live model list. */
export const STT_SUGGESTIONS: Partial<Record<ProviderId, Suggestion[]>> = {
  openrouter: [
    { id: "openai/gpt-transcribe", label: "GPT Transcribe", tags: ["best", "fast"] },
    { id: "openai/gpt-4o-mini-transcribe", label: "GPT-4o mini Transcribe", tags: ["fast"] },
    { id: "mistralai/voxtral-mini-transcribe-3-0", label: "Voxtral Mini Transcribe 3", tags: ["fast"] },
    { id: "openai/whisper-large-v3-turbo", label: "Whisper Large v3 Turbo", tags: ["cheap"] },
    { id: "elevenlabs/scribe-v2", label: "ElevenLabs Scribe v2", tags: ["accurate"] },
    { id: "microsoft/mai-transcribe-2", label: "MAI-Transcribe 2", tags: ["multilingual"] },
    { id: "google/gemini-3.5-transcribe", label: "Gemini 3.5 Transcribe" },
    { id: "deepgram/nova-3", label: "Deepgram Nova-3" },
    { id: "nvidia/parakeet-tdt-0.6b-v3", label: "NVIDIA Parakeet TDT v3 (English & European languages)", tags: ["fastest"] },
  ],
  openai: [
    { id: "gpt-transcribe", label: "GPT Transcribe", tags: ["best"] },
    { id: "gpt-4o-mini-transcribe", label: "GPT-4o mini Transcribe", tags: ["fast"] },
    { id: "whisper-1", label: "Whisper", tags: ["cheap"] },
  ],
  groq: [
    { id: "whisper-large-v3-turbo", label: "Whisper Large v3 Turbo", tags: ["fastest"] },
    { id: "whisper-large-v3", label: "Whisper Large v3", tags: ["accurate"] },
  ],
  mistral: [{ id: "voxtral-mini-latest", label: "Voxtral Mini", tags: ["fast"] }],
  custom: [
    { id: "whisper-1", label: "whisper-1" },
    { id: "Systran/faster-whisper-large-v3", label: "faster-whisper large-v3 (speaches)" },
  ],
};

export const LLM_SUGGESTIONS: Partial<Record<ProviderId, Suggestion[]>> = {
  openrouter: [
    { id: "openai/gpt-6-luna", label: "GPT-6 Luna", tags: ["fast", "cheap"] },
    { id: "anthropic/claude-haiku-5.5", label: "Claude Haiku 5.5", tags: ["fast", "smart"] },
    { id: "google/gemini-3.5-flash-lite", label: "Gemini 3.5 Flash Lite", tags: ["fastest", "cheap"] },
    { id: "google/gemini-3.8-flash", label: "Gemini 3.8 Flash", tags: ["smart"] },
    { id: "deepseek/deepseek-v4.1-flash", label: "DeepSeek V4.1 Flash", tags: ["cheap"] },
    { id: "openai/gpt-6.1-terra", label: "GPT-6.1 Terra", tags: ["smart"] },
    { id: "anthropic/claude-sonnet-5.5", label: "Claude Sonnet 5.5", tags: ["best"] },
  ],
  openai: [
    { id: "gpt-6-luna", label: "GPT-6 Luna", tags: ["fast", "cheap"] },
    { id: "gpt-6.1-terra", label: "GPT-6.1 Terra", tags: ["smart"] },
    { id: "gpt-6.1-sol", label: "GPT-6.1 Sol", tags: ["best"] },
  ],
  anthropic: [
    { id: "claude-haiku-5-5", label: "Claude Haiku 5.5", tags: ["fast"] },
    { id: "claude-sonnet-5-5", label: "Claude Sonnet 5.5", tags: ["smart"] },
    { id: "claude-opus-5-5", label: "Claude Opus 5.5", tags: ["best"] },
  ],
  gemini: [
    { id: "gemini-3.5-flash-lite", label: "Gemini 3.5 Flash Lite", tags: ["fastest"] },
    { id: "gemini-3.8-flash", label: "Gemini 3.8 Flash", tags: ["smart"] },
  ],
  mistral: [
    { id: "mistral-small-latest", label: "Mistral Small", tags: ["fast"] },
    { id: "mistral-large-latest", label: "Mistral Large", tags: ["smart"] },
  ],
  groq: [],
  custom: [],
};

export const LANGUAGES: { code: string; name: string }[] = [
  { code: "auto", name: "Auto-detect" },
  { code: "en", name: "English" },
  { code: "tr", name: "Turkish" },
  { code: "de", name: "German" },
  { code: "fr", name: "French" },
  { code: "es", name: "Spanish" },
  { code: "it", name: "Italian" },
  { code: "pt", name: "Portuguese" },
  { code: "nl", name: "Dutch" },
  { code: "pl", name: "Polish" },
  { code: "ru", name: "Russian" },
  { code: "uk", name: "Ukrainian" },
  { code: "ar", name: "Arabic" },
  { code: "fa", name: "Persian" },
  { code: "he", name: "Hebrew" },
  { code: "hi", name: "Hindi" },
  { code: "bn", name: "Bengali" },
  { code: "ur", name: "Urdu" },
  { code: "zh", name: "Chinese" },
  { code: "ja", name: "Japanese" },
  { code: "ko", name: "Korean" },
  { code: "vi", name: "Vietnamese" },
  { code: "th", name: "Thai" },
  { code: "id", name: "Indonesian" },
  { code: "ms", name: "Malay" },
  { code: "tl", name: "Filipino" },
  { code: "sv", name: "Swedish" },
  { code: "no", name: "Norwegian" },
  { code: "da", name: "Danish" },
  { code: "fi", name: "Finnish" },
  { code: "cs", name: "Czech" },
  { code: "sk", name: "Slovak" },
  { code: "hu", name: "Hungarian" },
  { code: "ro", name: "Romanian" },
  { code: "bg", name: "Bulgarian" },
  { code: "el", name: "Greek" },
  { code: "sr", name: "Serbian" },
  { code: "hr", name: "Croatian" },
  { code: "ca", name: "Catalan" },
  { code: "az", name: "Azerbaijani" },
  { code: "kk", name: "Kazakh" },
  { code: "sw", name: "Swahili" },
];

export const TAG_STYLE: Record<string, string> = {
  fast: "bg-emerald-500/15 text-emerald-300",
  fastest: "bg-emerald-500/15 text-emerald-300",
  cheap: "bg-sky-500/15 text-sky-300",
  accurate: "bg-violet-500/15 text-violet-300",
  smart: "bg-violet-500/15 text-violet-300",
  best: "bg-amber-500/15 text-amber-300",
  multilingual: "bg-fuchsia-500/15 text-fuchsia-300",
};
