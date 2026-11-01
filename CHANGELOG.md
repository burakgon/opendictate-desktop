# Changelog

All notable changes to OpenDictate are documented here.

## [0.2.2] - 2026-11-01

### macOS
- Signed with a Developer ID and notarized by Apple: OpenDictate opens without the "unidentified developer" warning, and macOS now keeps its Accessibility and microphone permissions across updates. Coming from 0.2.1 or earlier (unsigned), allow OpenDictate under *Accessibility* once more after this update.

## [0.2.1] - 2026-11-01

### Changed
- Claude Haiku 5.5 (`anthropic/claude-haiku-5.5` on OpenRouter) is the default model for smart formatting and voice commands, and the first suggestion in the model pickers. Models you already chose are kept.

### Fixed
- Links in the app window did nothing: *Get a key* on the provider pages and in the setup, *What's new* on the update banner and the GitHub button.
- Drop-down lists such as the model and language pickers were partly hidden behind the next section of the page, which also caught the clicks meant for them.

## [0.2.0] - 2026-11-01

### Updates
- OpenDictate updates itself: once a day it looks at the latest GitHub release, and on macOS, Windows and the Linux AppImage one click downloads the update, checks its signature, installs it and restarts. Also from the tray / menu bar menu, and *Settings → Interface → Check now*. Other packages (deb, rpm, AUR) link to the release page.

### macOS
- OpenDictate is now a native menu bar app: no Dock icon until its window is opened, a monochrome menu bar icon that follows light and dark menu bars, a standard app menu (⌘, ⌘W, ⌘Q, Edit menu for text fields) and the window with native traffic lights, corners and shadow. Clicking the app in Finder or Spotlight opens the window.
- The overlay is a native HUD (AppKit and Core Animation) instead of a web view: it never takes keyboard focus, floats above full-screen apps and on every Space, survives "Hide OpenDictate", and follows the screen you are working on.
- Single-key shortcuts: Right ⌘ (the new default: tap to dictate, hold or double-tap for a command), Right ⌥, fn, or a mouse side button. Key combinations keep using system hotkeys, so they also work in secure text fields. Esc cancels, and is only grabbed while OpenDictate is busy.
- Pasting uses the keyboard layout you type with (Dvorak, AZERTY, …) and modifier keys you still hold no longer turn ⌘V into another shortcut.
- The clipboard is restored with everything that was on it — images, files, rich text — and left alone if another app changed it in the meantime.
- The selected text is read through the accessibility API when the app supports it, without pressing ⌘C or touching the clipboard.
- Permissions: a macOS permissions section in Settings and in the first-run setup, with live status and one-click prompts for Accessibility and the microphone; OpenDictate no longer asks for Accessibility the moment it starts, and explains a missing microphone permission instead of recording silence.
- `opendictate://` URLs (toggle, command, action/<id>, show/<page>, …) for Shortcuts, Raycast, Alfred and scripts.
- Lighter: about 20 MB of memory in the background (no web view at all until the window is opened, and its content process is released when it is closed), UI sounds through NSSound, no App Nap delays while dictating.
- The app bundle is ad-hoc signed as a whole with the hardened runtime and the microphone entitlement.
- `npm run release:mac` builds a universal app and DMG, signed with a Developer ID and notarized when the credentials are
  available; GitHub releases do the same once the signing secrets are set (see CONTRIBUTING.md).

### Fixed
- The onboarding no longer jumps back to the start after granting Linux permissions on the permissions step.

## [0.1.1] - 2026-10-28

### Added
- Optionally pause music and videos (MPRIS players) while recording, and resume them afterwards.
- "Update available" banner from a once-a-day check of the latest GitHub release (can be turned off).
- Desktop notifications for errors when the overlay is turned off or unavailable.
- Clear message when the microphone delivers digital silence (muted or wrong input).

### Changed
- Rate-limited (429) and transient upstream errors are retried automatically with backoff.
- The main window is created on first use: OpenDictate started in the tray (e.g. at login) no longer loads it, saving ~200 MB of memory.

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
