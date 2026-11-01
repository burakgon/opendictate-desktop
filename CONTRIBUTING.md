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

On macOS, `npm run tauri build -- --bundles app` builds `OpenDictate.app`. Running its binary from a terminal
(`src-tauri/target/release/bundle/macos/OpenDictate.app/Contents/MacOS/opendictate`) is handy for development: it
inherits the terminal's Accessibility and microphone permissions, whereas the app opened from Finder has its own.

### Handy environment variables

| Variable | Effect |
| --- | --- |
| `RUST_LOG=opendictate_lib=debug` | Verbose logs |
| `OPENDICTATE_TEST_AUDIO=/path/to.wav` | Use this WAV instead of the microphone (it is never opened) - for testing the pipeline |
| `OPENDICTATE_NO_LAYER_SHELL=1` | Show the overlay as a regular window instead of a Wayland layer surface |

## Releasing

1. Add a `## [x.y.z] - YYYY-MM-DD` section to `CHANGELOG.md` (it becomes the release notes) and commit it.
2. `npm run release -- x.y.z` sets the version in every manifest, runs the checks, commits, tags `vx.y.z` and pushes.
3. GitHub Actions builds Linux, macOS (Apple Silicon and Intel) and Windows, uploads the installers together with signed
   update files and `latest.json`, and publishes the release when every build has succeeded. Running copies of OpenDictate
   (0.2.0 and later) then offer the update.

Updates are signed with the key in the `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` secrets; the
matching public key is in `tauri.conf.json`. **Keep a backup of that private key**: without it, installed copies can no
longer be updated automatically.

## Signed macOS builds

`npm run release:mac` (`scripts/macos-release.sh`) builds a universal OpenDictate.app (Apple Silicon + Intel) and a DMG
into `src-tauri/target/release/macos-release/`, signs them with the first **Developer ID Application** identity in your
keychain (or `APPLE_SIGNING_IDENTITY`), and notarizes and staples both when it finds credentials. Without a Developer ID it
falls back to ad-hoc signing. It refuses certificates that Apple has revoked: macOS blocks apps signed with them.

1. **Certificate.** Xcode → Settings → Accounts → your team → *Manage Certificates* → **+** → *Developer ID Application*
   (only the Account Holder, or an Admin they allowed, can create one).
2. **Notarization credentials**, one of:
   - an app-specific password stored once: `xcrun notarytool store-credentials opendictate --apple-id <apple id> --team-id <team id>`,
     then `NOTARY_PROFILE=opendictate npm run release:mac`;
   - an App Store Connect API key (*Users and Access → Integrations*): `APPLE_API_KEY=<key id> APPLE_API_ISSUER=<issuer id>
     APPLE_API_KEY_PATH=/path/AuthKey_<key id>.p8 npm run release:mac`.

The result is checked with Gatekeeper (`spctl`) at the end. A Developer ID signature also means macOS keeps the
Accessibility and microphone permissions across updates.

**Releases on GitHub** are signed and notarized the same way when these repository secrets exist (otherwise they are
ad-hoc signed):

| Secret | Value |
| --- | --- |
| `APPLE_CERTIFICATE` | the Developer ID identity exported from Keychain Access as `.p12`, base64-encoded (`base64 -i cert.p12`) |
| `APPLE_CERTIFICATE_PASSWORD` | the password chosen when exporting the `.p12` |
| `APPLE_SIGNING_IDENTITY` | e.g. `Developer ID Application: Your Name (TEAMID)` |
| `APPLE_API_KEY`, `APPLE_API_ISSUER` | App Store Connect API key ID and issuer ID (for notarization) |
| `APPLE_API_KEY_P8` | the contents of the `AuthKey_<key id>.p8` file |

## Guidelines

- Keep the app fast: nothing should block the hotkey → record → insert path.
- Keep it private: never send data anywhere except the provider the user configured.
- Match the existing code style; run the checks above before opening a PR.
- For UI changes, please include a screenshot.

## Reporting bugs

Please include your OS, desktop environment (and whether you use Wayland or X11), the provider/model you use,
and the output of `RUST_LOG=opendictate_lib=debug opendictate` around the problem — but **remove your API keys** first.
