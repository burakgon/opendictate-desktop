# Changelog

All notable changes to OpenDictate are documented here.

## [0.1.0] - 2026-10-28

First public release.

### Dictation
- Tap your hotkey to dictate into any application; tap again to insert the text at the cursor.
- Hold the hotkey (or double-tap it) to speak a **voice command** that rewrites the selected text, or writes new text when nothing is selected.
- **Smart formatting** (optional): punctuation, filler-word removal and spoken formatting such as "new paragraph", bullet lists, "make that bold", "put this in a table" and "scratch that", in any language.
- **Command word**: start a dictation with a word of your choice ("Jarvis, make this formal") to run it as a command.
- **Quick actions** for the selected text (Fix grammar, Make formal, Translate to English, …) from the tray, the app or the CLI.
- Keep talking while the previous dictation is processed; results are inserted in order.
- Failed transcriptions keep the recording so it can be retried.

### Models
- OpenRouter, OpenAI, Groq, Mistral, Google Gemini, Anthropic and any OpenAI-compatible server (local Whisper servers, Ollama, LM Studio, vLLM).
- Live model catalogues with recommended picks and token prices; models are asked not to "think" for snappy answers, stepping up automatically when reasoning is mandatory.

### Desktop integration
- Floating overlay with a live waveform; a true layer-shell surface on Wayland that never steals focus.
- Linux hotkeys through evdev (any key, including a lone Right Ctrl, mouse side buttons and the Copilot key) or the XDG GlobalShortcuts portal; native shortcuts on macOS and Windows.
- Terminal-aware pasting (Ctrl+Shift+V) on KDE Plasma, Hyprland, Sway and X11.
- Clipboard restored after pasting; transient clipboard contents hidden from clipboard managers.
- One-click Linux permission setup (no log-out needed), tray menu, history with search, usage stats, and a scriptable CLI.
