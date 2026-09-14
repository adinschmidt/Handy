# Superwhisper compatibility work

## Status and source history

Superwhisper Scribe and S1-Voice are implemented as an opt-in cloud provider for a personal licensed account. Models settings accepts the device ID, license ID, and signature as one credential set. The compact selector, general settings summary, and tray support the provider. Clearing credentials switches an active Superwhisper provider back to Local.

A September 6, 2026 disposable live probe confirmed that the proxy accepts Handy's existing 16 kHz mono PCM16 WAV encoding and returns the expected phrase. No Opus encoder is required. See the dated evidence in [Research findings](RESEARCH_FINDINGS.md).

`src-tauri/src/settings/superwhisper.rs` validates and atomically updates the three secret-map entries. `src-tauri/src/transcription_provider/superwhisper.rs` implements the multipart request, bounded vocabulary hints, response parsing, and audio-event protection. Its ignored `live_wav_compatibility_probe` test accepts `SW_PROBE_WAV`, `SW_PROBE_EXPECTED`, and the three `SW_X_*` environment variables. It re-encodes 16 kHz mono PCM16 input with Handy's WAV encoder and sends one request. Run it only with explicit authorization and disposable audio.

On macOS, **Import from Superwhisper** reads the local Superwhisper request cache and saves the newest valid credential set. It scans only known Superwhisper endpoints and never launches Superwhisper or sends a request. If no usable request is cached, transcribe something in Superwhisper and retry. Cached credentials may have expired; importing does not verify account access. Linux retains manual credential entry.

Requests on every OS use the captured `X-Platform: macos` and Superwhisper 2.16.6 User-Agent, including its macOS 26.5.2 version string. These fixed compatibility headers do not come from the current installation. HTTP and TLS still use Handy's reqwest client.

Credentials use Handy's existing local settings file, not an OS keychain. The credential form masks all three values. Invalid or incomplete sets cannot be saved or selected, and HTTP errors omit response bodies. Diarization remains out of scope. Scribe audio-event tagging has a Default/On/Off control. Default omits `tag_audio_events`; On and Off send `true` and `false`. Manual recordings returned clap, sigh, and throat-clearing tags. Explicit Off behavior has not yet been compared. Disable Voice Activity Detection during comparison recordings so local filtering does not remove non-speech sounds before upload.

This checkout is the `adinschmidt/Handy` fork of `cjpais/Handy`:

- `origin`: https://github.com/adinschmidt/Handy
- `upstream`: https://github.com/cjpais/Handy
- Continuation branch: `work/superwhisper-continuation`
- Fork base before the upstream update: `074acc2abc84004bdf6233d209a8567b3e4404d3`
- Upstream snapshot fetched September 6, 2026: `bc7face`, on `upstream/main`.

The fork already implements Codex ASR and direct ElevenLabs Scribe. Preserve these providers when adding Superwhisper. Upstream changed tray updates, shortcut activation, and language-aware transcript cleanup after the original investigation; the July implementation plan's source snapshot predates these changes.

## S1-Voice

The model selector beneath audio-event tagging defaults to Scribe for existing settings. S1-Voice uses the same saved credentials. The persisted provider ID remains `superwhisper_scribe` for compatibility; `superwhisper_model` selects the transport.

`src-tauri/src/transcription_provider/superwhisper_s1.rs` discovers the default region with `GET /v2/inference/regions`, then requests `POST /v2/inference/key?region=<id>` with an empty JSON object using the three device/license headers. Each transcription obtains a fresh token without persisting it. Only HTTPS hosts beneath `superwhisper.com` can receive that token.

The regional `POST /generate` request sends multipart WAV in `audio`, `language`, vocabulary hints as a comma-separated `asr_prompt`, and `enable_word_timestamps=true`. It sends `enable_audio_vocab=false` because Handy does not build Superwhisper vocabulary files. It reads the top-level `text` field. Audio-event tagging applies only to Scribe, so its control is disabled for S1-Voice without discarding the saved choice.

September 13, 2026 cache inspection confirmed that the key response contains `key`, and region discovery returns `regions` entries with `id` and `host`, plus a `default` region ID. The cached key request uses `X-ID`, `X-License`, and `X-Signature`, correcting the earlier inferred license-bearer authentication in the historical specification. Region discovery supplied AWS regional hosts. The current S1 request uses `/generate`, distinct from the older `/v1/c/run` capture. Live probes through Handy confirmed 16 kHz mono PCM16 WAV acceptance and exact transcription of disposable speech with both explicit English and automatic language detection.

## Language-model post-processing

Select Superwhisper in the post-processing provider menu to reuse the credentials saved in Models. Its model menu offers Gemini 3.7 Flash, GPT-5.6 Luna, and Sonnet 5. Refresh checks the account's cloud catalog and removes deprecated or unavailable entries from those choices. Existing post-processing provider selections remain unchanged.

`src-tauri/src/superwhisper_llm.rs` sends the selected prompt after substituting `${output}` with the transcript. All three endpoints accept `model`, `messages`, and `stream`. The OpenAI path uses `max_completion_tokens`; Gemini and Anthropic use `max_tokens`. Handy requests an 8,192-token output budget and does not assume structured-output support.

Superwhisper streams replies even when a request specifies `stream=false`. The adapter collects OpenAI, Gemini, or Anthropic text events and requires a successful finish signal. It excludes Gemini thought parts and Anthropic thinking events, removes an outer `sw_response_content` wrapper, and rejects incomplete, truncated, empty, or failed responses. On failure, the existing post-processing pipeline keeps the original transcript. Error messages omit response bodies and credentials.

September 13, 2026 live probes through Handy's post-processing function returned the expected disposable corrected sentence for all three models. No separate OpenAI, Google, or Anthropic key was used.

## Reference material

Read these in order:

1. [Research findings](RESEARCH_FINDINGS.md). Historical evidence, request headers, multipart bodies, response shapes, and replay results. Evidence labels distinguish captured facts from inference.
2. [Implementation plan](IMPLEMENTATION_PLAN.md). The original Handy integration design, credential handling, audio-format decision, and acceptance criteria. Recheck code locations against this checkout.
3. [OpenAPI specification](superwhisper-api.openapi.yaml). Unofficial API description, also viewable with [Swagger UI](superwhisper-swagger.html).
4. [Replay templates](replay-templates.sh). Credential-free request examples.
5. [Transcription script](transcribe.sh). Standalone proxy/direct transcription client with text, JSON, SRT, and VTT output. Running it with audio sends a real request.

Keep the original findings and plan as historical references. Add dated evidence when a new probe confirms or changes a finding. The original artifact copies also remain at `/Users/adin/Documents/Codex/2026-07-27/usin/outputs/`.

## Protocol and scope

The provider uses the existing recording and output pipeline in `src-tauri/src/transcription_provider/`. The Scribe transport follows the direct ElevenLabs provider while keeping Superwhisper's authentication and multipart fields separate.

The July replay verified `POST https://api.superwhisper.com/elevenlabs/v1/transcribe` with a multipart audio file and the `X-ID`, `X-License`, `X-Signature`, and `X-Platform: macos` headers. The observed credential set worked across different bodies and paths. Signature derivation remains unknown. The September 6 probes confirmed validity of the tested credential set.

The July capture used Ogg Opus. The September 6 live probe confirmed WAV acceptance, so the provider uses Handy's existing encoder.

Keep license activation, sync, background polling, and account management out of scope.

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
