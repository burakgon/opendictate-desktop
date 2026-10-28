# Contributing to OpenDictate

Thanks for helping! Bug reports, ideas and pull requests are all welcome.

## Development setup

1. Install [Rust](https://rustup.rs), [Node.js](https://nodejs.org) 20+ and the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/).
   On Linux you also need the ALSA development headers (`libasound2-dev` on Debian/Ubuntu, `alsa-lib` on Arch).
2. `npm install`
3. `npm run tauri dev` starts the app with hot reload for the UI.

Useful commands:

```sh
npm run typecheck                                   # TypeScript
cargo test --manifest-path src-tauri/Cargo.toml     # Rust unit tests
cargo clippy --manifest-path src-tauri/Cargo.toml   # lints
```

Set `RUST_LOG=opendictate_lib=debug` to see what the engine is doing (hotkey events, timings, …).

### Handy environment variables

| Variable | Effect |
| --- | --- |
| `RUST_LOG=opendictate_lib=debug` | Verbose logs |
| `OPENDICTATE_TEST_AUDIO=/path/to.wav` | Use this WAV instead of the microphone recording (for testing the pipeline) |
| `OPENDICTATE_NO_LAYER_SHELL=1` | Show the overlay as a regular window instead of a Wayland layer surface |

## Guidelines

- Keep the app fast: nothing should block the hotkey → record → insert path.
- Keep it private: never send data anywhere except the provider the user configured.
- Match the existing code style; run the checks above before opening a PR.
- For UI changes, please include a screenshot.

## Reporting bugs

Please include your OS, desktop environment (and whether you use Wayland or X11), the provider/model you use,
and the output of `RUST_LOG=opendictate_lib=debug opendictate` around the problem — but **remove your API keys** first.
