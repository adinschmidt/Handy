# Superwhisper compatibility work

## Status and source history

Superwhisper support is not implemented in Handy yet. The July 27-28, 2026 investigation recovered the request format and demonstrated a successful transcription replay. The intended first implementation is an opt-in Scribe provider for a personal licensed Superwhisper account, initially for Linux.

This checkout is the `adinschmidt/Handy` fork of `cjpais/Handy`:

- `origin`: https://github.com/adinschmidt/Handy
- `upstream`: https://github.com/cjpais/Handy
- Continuation branch: `work/superwhisper-continuation`
- Fork base before the upstream update: `074acc2abc84004bdf6233d209a8567b3e4404d3`
- Upstream snapshot fetched September 6, 2026: `bc7face`, on `upstream/main`.

The fork already implements Codex ASR and direct ElevenLabs Scribe. Preserve these providers when adding Superwhisper. Upstream changed tray updates, shortcut activation, and language-aware transcript cleanup after the original investigation; the July implementation plan's source snapshot predates these changes.

## Reference material

Read these in order:

1. [Research findings](RESEARCH_FINDINGS.md). Historical evidence, request headers, multipart bodies, response shapes, and replay results. Evidence labels distinguish captured facts from inference.
2. [Implementation plan](IMPLEMENTATION_PLAN.md). The original Handy integration design, credential handling, audio-format decision, and acceptance criteria. Recheck code locations against this checkout.
3. [OpenAPI specification](superwhisper-api.openapi.yaml). Unofficial API description, also viewable with [Swagger UI](superwhisper-swagger.html).
4. [Replay templates](replay-templates.sh). Credential-free request examples.
5. [Transcription script](transcribe.sh). Standalone proxy/direct transcription client with text, JSON, SRT, and VTT output. Running it with audio sends a real request.

Keep the original findings and plan as historical references. Add dated evidence when a new probe confirms or changes a finding. The original artifact copies also remain at `/Users/adin/Documents/Codex/2026-07-27/usin/outputs/`.

## First implementation

Add a provider to the existing pipeline in `src-tauri/src/transcription_provider/`, using `elevenlabs.rs` as the closest reference. Do not create another recording or output pipeline.

The July replay verified `POST https://api.superwhisper.com/elevenlabs/v1/transcribe` with a multipart audio file and the `X-ID`, `X-License`, `X-Signature`, and `X-Platform: macos` headers. The observed credential set worked across different bodies and paths. Signature derivation and current credential validity remain unverified.

The first unresolved protocol question is whether the proxy accepts Handy's existing WAV encoding. The captured request used Ogg Opus. Resolve that with a single explicitly authorized disposable-audio probe before adding an encoder dependency. Mock tests cannot establish server acceptance of an audio format.

Keep S1 Ultra and its short-lived JWT acquisition/refresh out of the first implementation. Keep license activation, sync, background polling, and account management out of scope.

### Integration points

- `src-tauri/src/settings.rs`: provider enum, defaults, secret settings, migrations.
- `src-tauri/src/transcription_provider/mod.rs`: dispatch, shared HTTP client, cancellation-compatible requests, transcript cleanup.
- `src-tauri/src/transcription_provider/elevenlabs.rs`: multipart WAV transport and audio-event preservation.
- `src-tauri/src/commands/transcription.rs`: provider configuration commands.
- `src/components/settings/models/ModelsSettings.tsx`, `src/stores/settingsStore.ts`, and `src/hooks/useSettings.ts`: cloud-provider settings.
- `src-tauri/src/tray.rs`: provider selection must participate in `MenuInputs` so tray changes invalidate the cached menu.
- `src/bindings.ts`: generated Tauri types. Keep aligned with Rust settings and commands.
- `src/i18n/locales/en/translation.json`: new user-facing strings.

### Verification

Install frontend dependencies with `bun install --frozen-lockfile`. Run `bun run build` and `bun run lint` for frontend changes. Run `cargo check --tests` and focused `cargo test --lib transcription_provider` from `src-tauri` for provider changes, plus settings/tray tests when touching those integrations. Follow `BUILD.md` for native dependencies.

Keep credentials outside the repository. The standalone script can read environment variables or `~/.config/superwhisper-replay/env`; never copy that file into this directory. Do not log request headers or response bodies containing private transcripts. Live transcription sends audio and may consume account usage, so obtain explicit authorization for the recording and request. Build and test without launching the installed Handy app or writing its settings.
