# Superwhisper compatibility work

## Status and source history

Superwhisper Scribe is implemented as an opt-in cloud provider for a personal licensed account. Models settings accepts the device ID, license ID, and signature as one credential set. The compact selector, general settings summary, and tray support the provider. Clearing credentials switches an active Superwhisper provider back to Local.

A September 6, 2026 disposable live probe confirmed that the proxy accepts Handy's existing 16 kHz mono PCM16 WAV encoding and returns the expected phrase. No Opus encoder is required. See the dated evidence in [Research findings](RESEARCH_FINDINGS.md).

`src-tauri/src/settings/superwhisper.rs` validates and atomically updates the three secret-map entries. `src-tauri/src/transcription_provider/superwhisper.rs` implements the multipart request, bounded vocabulary hints, response parsing, and audio-event protection. Its ignored `live_wav_compatibility_probe` test accepts `SW_PROBE_WAV`, `SW_PROBE_EXPECTED`, and the three `SW_X_*` environment variables. It re-encodes 16 kHz mono PCM16 input with Handy's WAV encoder and sends one request. Run it only with explicit authorization and disposable audio.

On macOS, **Import from Superwhisper** reads the local Superwhisper request cache and saves the newest valid credential set. It scans only known Superwhisper endpoints and never launches Superwhisper or sends a request. If no usable request is cached, transcribe something in Superwhisper and retry. Cached credentials may have expired; importing does not verify account access. Linux retains manual credential entry.

Requests on every OS use the captured `X-Platform: macos` and Superwhisper 2.16.6 User-Agent, including its macOS 26.5.2 version string. These fixed compatibility headers do not come from the current installation. HTTP and TLS still use Handy's reqwest client.

Credentials use Handy's existing local settings file, not an OS keychain. The credential form masks all three values. Invalid or incomplete sets cannot be saved or selected, and HTTP errors omit response bodies. S1 and diarization remain out of scope. Audio-event tagging has a Default/On/Off control. Default omits `tag_audio_events`; On and Off send `true` and `false`. Manual recordings returned clap, sigh, and throat-clearing tags. Explicit Off behavior has not yet been compared. Disable Voice Activity Detection during comparison recordings so local filtering does not remove non-speech sounds before upload.

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

## Protocol and scope

The provider uses the existing recording and output pipeline in `src-tauri/src/transcription_provider/`. Its transport follows the direct ElevenLabs provider while keeping Superwhisper's authentication and multipart fields separate.

The July replay verified `POST https://api.superwhisper.com/elevenlabs/v1/transcribe` with a multipart audio file and the `X-ID`, `X-License`, `X-Signature`, and `X-Platform: macos` headers. The observed credential set worked across different bodies and paths. Signature derivation remains unknown. The September 6 probes confirmed validity of the tested credential set.

The July capture used Ogg Opus. The September 6 live probe confirmed WAV acceptance, so the provider uses Handy's existing encoder.

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

Generate Tauri bindings without launching the app with `cargo test --lib export_typescript_bindings -- --ignored` from `src-tauri`.

Keep credentials outside the repository. The standalone script can read environment variables or `~/.config/superwhisper-replay/env`; never copy that file into this directory. Do not log request headers or response bodies containing private transcripts. Live transcription sends audio and may consume account usage, so obtain explicit authorization for the recording and request. Build and test without launching the installed Handy app or writing its settings.
