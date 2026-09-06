# Handy personal Superwhisper compatibility: implementation handoff

Status: planning document only  
Prepared: 2026-07-28  
Handy source snapshot inspected: `074acc2abc84004bdf6233d209a8567b3e4404d3`  
Intended use: a private, opt-in Linux stopgap for the user's own licensed Superwhisper account

## Read this first

This directory contains:

- `IMPLEMENTATION_PLAN.md` — this Handy-specific handoff.
- `RESEARCH_FINDINGS.md` — the full request investigation, evidence labels, curl verification, and limitations.
- `superwhisper-api.openapi.yaml` — an unofficial OpenAPI 3.1 description of observed and inferred Superwhisper interfaces.
- `replay-templates.sh` — credential-free curl templates. It sends nothing merely by being run.
- `superwhisper-swagger.html` — a local Swagger UI shell for `superwhisper-api.openapi.yaml`.

None of these files contains a real device ID, license ID, signature, JWT, API key, transcript, or recording. Do not add any such value to the repository.

The target is a personal compatibility path, not an official Superwhisper integration. Keep it local unless Superwhisper authorizes the integration and Handy's feature-freeze contribution process is followed.

## Outcome

Implement one new, explicit cloud transcription target in Handy:

```text
Superwhisper proxy (Scribe)
```

When selected, Handy should:

1. Record audio through its existing pipeline.
2. Encode the completed recording in a server-accepted format.
3. Send exactly one user-triggered multipart request to Superwhisper's Scribe proxy.
4. Parse the transcript and pass it through Handy's existing cloud transcript cleanup/output flow.
5. Never log, export, or commit the three Superwhisper credential values.

Do not include automated polling, background traffic, account management, license activation, file sync, statistics, chat, or bulk transcription.

S1 Ultra should be treated as a later phase. Its transcription request body is known, but robust acquisition and refresh of its short-lived inference JWT is not yet reconstructed.

## Important discovery in current Handy

Handy already has the right abstraction:

- `src-tauri/src/settings.rs` defines `TranscriptionProvider`.
- `src-tauri/src/transcription_provider/mod.rs` routes completed audio to local, Codex ASR, or ElevenLabs.
- `src-tauri/src/transcription_provider/elevenlabs.rs` already implements direct ElevenLabs Scribe v2, WAV encoding, language normalization, `tag_audio_events`, response parsing, audio-event protection, and wiremock tests.
- `src-tauri/src/audio_toolkit/audio/utils.rs` exposes `encode_wav_bytes`.
- The settings UI, model selector, tray, Zustand store, Tauri commands, generated bindings, and i18n already know about cloud providers.

Do not create a second transcription pipeline. Add a provider to the existing one.

Handy's existing `ElevenlabsScribe` provider is a separate option:

| Handy target | Credentials | Destination | Audio-event control |
|---|---|---|---|
| Existing ElevenLabs Scribe | User's ElevenLabs API key | `api.elevenlabs.io` | Already implemented and explicit |
| Proposed Superwhisper proxy | Current Superwhisper device/license header set | `api.superwhisper.com` | Batch field not observed; do not assume |

If the user has an ElevenLabs API key, the existing provider may already be the cleanest way to use Scribe on Linux. The proposed provider is for using the user's Superwhisper-authorized path.

## Verified Superwhisper request

The following request family was recovered from Superwhisper 2.16.6 and successfully replayed with curl using a fresh disposable recording:

```http
POST https://api.superwhisper.com/elevenlabs/v1/transcribe
Accept: */*
Accept-Encoding: gzip, deflate, br
Accept-Language: en-CA,en-US;q=0.9,en;q=0.8
User-Agent: superwhisper/2.16.6 (com.superduper.superwhisper; build:2.16.6; macOS 26.5.2) Alamofire/5.8.0
X-ID: <device UUID>
X-License: <license UUID>
X-Platform: macos
X-Signature: <64-character lowercase hex value>
Content-Type: multipart/form-data; boundary=<generated for this request>
```

Verified multipart fields:

```text
file=@audio.ogg; type=audio/ogg; filename=audio.ogg
language_code=en
keyterms[]=Superwhisper
```

`keyterms[]` is repeatable and optional. The installed binary also supports optional `diarize=true|false`; it was not present in the captured request.

Let the HTTP library generate `Host`, `Content-Length`, and the multipart boundary. Never copy those values from a capture.

The successful curl used the same `X-ID`, `X-License`, and `X-Signature` seen in cached requests while changing the audio, multipart boundary, content length, and request time. The signature is therefore an opaque reusable credential in the observed version, not a per-request body/path/timestamp signature. Its derivation is unknown and should not be reimplemented.

Treat all three values as secrets. There is no ElevenLabs API key in this proxied batch request.

### Verified response shape

```json
{
  "language_code": "eng",
  "language_probability": 1.0,
  "text": "Example transcript",
  "words": [
    {
      "text": "Example",
      "start": 0.0,
      "end": 0.5,
      "type": "word",
      "logprob": 0.0
    }
  ],
  "transcription_id": "opaque",
  "audio_duration_secs": 1.0
}
```

Only `text` is required by Handy's current output pipeline. Parse the other fields permissively so added or omitted metadata does not fail an otherwise valid transcript.

## Recommended implementation

### 1. Add the provider enum case

Add a narrowly named variant:

```rust
TranscriptionProvider::SuperwhisperScribe
```

Use the serialized value:

```text
superwhisper_scribe
```

Do not call it merely `Superwhisper`, because S1 uses a different host, body, and authentication family.

Update all exhaustive matches:

- `src-tauri/src/settings.rs`
- `src-tauri/src/transcription_provider/mod.rs`
- `src-tauri/src/commands/transcription.rs`
- `src-tauri/src/tray.rs`
- `src-tauri/src/lib.rs`
- frontend provider unions and selectors

Cloud providers should continue to unload the local model and disable live-stream handling through the existing behavior.

### 2. Represent the credential set

This provider needs three values, so do not squeeze them into one fake API-key string.

The smallest change consistent with current Handy is to keep three entries in the existing `SecretMap`:

```text
superwhisper_x_id
superwhisper_x_license
superwhisper_x_signature
```

Add a helper returning a validated credential struct:

```rust
struct SuperwhisperCredentials<'a> {
    x_id: &'a str,
    x_license: &'a str,
    x_signature: &'a str,
}
```

Validation:

- `X-ID`: nonblank; expected 36-character UUID syntax.
- `X-License`: nonblank; expected 36-character UUID syntax.
- `X-Signature`: exactly 64 lowercase hexadecimal characters.
- Trim surrounding whitespace before storage/use.
- Missing or malformed values must prevent provider selection and must not trigger a request.

Add one Tauri command that saves the complete set atomically, rather than three independent commands that can leave a half-configured provider.

Important limitation: Handy's current `SecretMap` redacts values in Rust `Debug` output, but it is serialized through `tauri-plugin-store` and exposed to the frontend settings object. It is not an OS-keychain abstraction. For a minimal private branch, following that existing convention is acceptable only if the limitation is made clear. A stronger keychain migration is separate scope and should cover all cloud credentials consistently.

Never:

- include credentials in an error string;
- log a built request with headers;
- include credentials in history entries or telemetry;
- prefill documentation, tests, fixtures, screenshots, or commits with real values.

### 3. Add `transcription_provider/superwhisper.rs`

Model it after `elevenlabs.rs`, but keep its protocol separate.

Suggested interface:

```rust
pub async fn transcribe(
    credentials: SuperwhisperCredentials<'_>,
    samples: &[f32],
    language: Option<&str>,
    keyterms: &[String],
) -> Result<ProviderTranscript>
```

Add an internal `transcribe_at` accepting a base URL for wiremock tests. Production constants:

```text
API_BASE_URL=https://api.superwhisper.com
TRANSCRIPT_PATH=elevenlabs/v1/transcribe
```

Build the request with Handy's shared `http_client()`. Use HTTP/2 negotiation normally; do not force a stale connection or copied pseudo-headers.

Explicit request headers:

```text
Accept: */*
Accept-Language: en-CA,en-US;q=0.9,en;q=0.8
User-Agent: superwhisper/2.16.6 (com.superduper.superwhisper; build:2.16.6; macOS 26.5.2) Alamofire/5.8.0
X-ID: configured value
X-License: configured value
X-Platform: macos
X-Signature: configured value
```

Use the captured `X-Platform: macos` initially because that is the curl-verified credential context. This is a compatibility value, not local OS detection.

For content encoding, either enable reqwest's gzip/deflate/Brotli response support and advertise all three, or let reqwest negotiate only encodings it can decode. Do not advertise `br` without Brotli decoding. Compression is transport metadata, not part of authentication.

### 4. Resolve audio format before wiring the UI

The curl-verified request uploaded Ogg Opus as `audio.ogg`. Current Handy cloud clients encode WAV and the current Cargo manifest has no Ogg/Opus encoder dependency.

Use this decision sequence:

1. Add a disposable wire-level integration probe, run only with the user's opt-in credentials, to test whether the proxy accepts Handy's existing mono WAV payload as:

   ```text
   file=@recording.wav; type=audio/wav; filename=recording.wav
   ```

2. If the live proxy accepts it and returns the correct transcript, keep WAV. Record that result in `RESEARCH_FINDINGS.md`.
3. If it rejects WAV, add an in-process Ogg Opus encoder. Do not make a system `ffmpeg` installation a runtime requirement for Handy.
4. Match the verified conservative format: mono, 16 kHz, Ogg Opus, approximately 32 kbps.

Do not silently send uncompressed base64 or a JSON audio body to this endpoint.

### 5. Map Handy settings to multipart fields

Language:

- `auto` means omit `language_code`.
- Otherwise begin with the base ISO-639-1 value used by Handy, such as `en`, not the three-letter code used by the direct ElevenLabs endpoint.
- Keep Superwhisper normalization separate from `elevenlabs::normalize_language`.

Key terms:

- Map `settings.custom_words` to repeated `keyterms[]` parts.
- Trim values, omit blanks, preserve order, and set a reasonable count/length cap.
- Do not join them into one comma-separated value.

Diarization:

- Leave it out in the first implementation.
- Add a setting only after a disposable request verifies both request behavior and response representation.

Audio events:

- The captured Superwhisper batch request did not contain `tag_audio_events`.
- Direct ElevenLabs Scribe supports the field and Handy's direct provider already exposes it.
- Superwhisper may rely on ElevenLabs' default, strip the field, or use a different server-side policy.
- Do not forward Handy's `elevenlabs_audio_events` setting to the Superwhisper proxy until a short controlled request proves the proxy accepts it and demonstrates both `true` and `false`.
- If audio events appear as `words[].type == "audio_event"`, reuse the protection behavior in `elevenlabs.rs` so cleanup does not remove them.

### 6. Parse safely

Define a provider-specific response structure with:

- required `text: String`;
- defaulted `words: Vec<SuperwhisperWord>`;
- optional/defaulted language, probability, transcription ID, and duration fields.

Reject an HTTP failure with the status only. Do not place the response body in user-facing errors because a server error could echo request or account data.

Suggested messages:

```text
Superwhisper credentials are incomplete.
Could not reach Superwhisper transcription: <network category>
Superwhisper transcription returned HTTP 401.
Superwhisper returned an invalid transcription response.
```

For 401/403, tell the user to refresh the three imported values from their authorized Mac installation. For 429, report a rate/concurrency limit without automatic retry. Avoid blind retries of transcription POSTs because they may double bill.

### 7. Add the UI without exposing values unnecessarily

Add a third cloud card under Models settings:

```text
Superwhisper proxy (personal compatibility)
```

Fields:

- Device ID
- License ID
- Signature

All three inputs should use password masking, `autoComplete="off"`, and a short warning:

```text
Uses credentials from your own authorized Superwhisper installation.
Stored using Handy's existing local settings store. Do not share or commit them.
```

Add the provider to:

- `src/components/settings/models/ModelsSettings.tsx`
- `src/components/settings/general/ModelSettingsCard.tsx`
- `src/components/model-selector/ModelSelector.tsx`
- `src/components/model-selector/ModelDropdown.tsx`
- `src/stores/settingsStore.ts`
- `src/hooks/useSettings.ts`
- `src/i18n/locales/en/translation.json`
- tray menu labels and selection handlers

All visible strings must use i18next. Regenerate `src/bindings.ts` through the existing Tauri Specta path; do not hand-maintain generated types as the final state.

Keep this provider opt-in and visually distinct from the official direct ElevenLabs provider.

## Test plan

### Rust unit/integration tests

Use wiremock and synthetic credentials only.

Required assertions:

1. Refuses an empty or malformed credential set before network I/O.
2. Sends all explicit captured headers:
   - `Accept`
   - `Accept-Language`
   - `User-Agent`
   - `X-ID`
   - `X-License`
   - `X-Platform`
   - `X-Signature`
3. Multipart contains the exact `file` field and correct filename/MIME type.
4. Sends `language_code` when configured and omits it for automatic detection.
5. Emits one `keyterms[]` part per nonblank custom word.
6. Does not send `diarize` or `tag_audio_events` in the initial implementation.
7. Parses the verified response shape and tolerates absent optional fields.
8. Protects returned `audio_event` words if the server supplies them.
9. HTTP and JSON errors contain no synthetic credential value or response body.
10. Existing local, Codex ASR, and direct ElevenLabs tests still pass.

Do not assert a fixed multipart boundary, content length, or HTTP/2 connection detail.

### Frontend tests/checks

- Provider cannot be selected until all three values validate.
- Clearing any credential while active falls back to Local or leaves a clear inactive error state.
- Password fields do not reveal values by default.
- Provider labels are correct in Models settings, compact selector, general settings summary, and tray.
- Existing direct ElevenLabs audio-event toggle remains scoped to direct ElevenLabs.

### One live acceptance test

After mock tests pass, perform one user-triggered live request with:

- the user's current authorized values entered locally;
- a newly recorded disposable phrase;
- no cached or private recording;
- no key term beyond a harmless synthetic word;
- debug logging inspected to confirm no credential leakage.

Expected result:

- HTTP 200;
- returned text matches the disposable phrase;
- exactly one transcription POST;
- no background Superwhisper requests;
- no permanent proxy, trusted certificate, packet filter, or system network change.

If WAV is accepted, document it. If not, stop after the single failure, implement Ogg Opus, and repeat once.

## S1 Ultra: phase 2 only

The observed S1 request is:

```http
POST https://ai.superwhisper.com/v1/c/run
Authorization: Bearer <short-lived inference JWT>
Content-Type: application/json
```

```json
{
  "translate": false,
  "audio_base64": "<base64 RIFF/WAVE bytes>",
  "prompt": "<vocabulary prompt>",
  "language": "en",
  "word_timestamps": true
}
```

This is replayable only while a captured JWT remains unexpired. Do not ship a provider that asks the user to paste and persist a short-lived JWT.

Before implementing S1:

1. Capture the exact authenticated request and response for `GET /v1/inference/key` or `GET /v2/inference/key?region=...`.
2. Confirm which device/license or bearer credential family protects that bootstrap call.
3. Capture region discovery and expiry/refresh behavior.
4. Determine the S1 response schema and error bodies with a disposable phrase.
5. Implement in-memory token caching with expiry-aware refresh and no token logging.
6. Add it as a separate `SuperwhisperS1Ultra` provider.

Until those items are complete, S1 should remain documented but disabled.

## Suggested implementation order

1. Read `AGENTS.md` and keep the work on a private branch. Do not open a PR during Handy's feature freeze without the documented community-feedback process.
2. Add credential types, validation, defaults, and settings tests.
3. Add the provider module with wiremock tests.
4. Run the one-request WAV compatibility probe.
5. Add Ogg Opus only if required.
6. Route the provider through `transcription_provider/mod.rs`.
7. Update provider selection commands, tray, and exhaustive matches.
8. Add frontend settings and model selectors.
9. Regenerate bindings and add i18n strings.
10. Run formatting, Rust tests/clippy, frontend lint/typecheck/build, then the single live acceptance test.

## Acceptance criteria

- A Linux user can explicitly select `SuperwhisperScribe` in Handy.
- A completed recording produces one verified-shape request and returns text through the normal Handy output path.
- The request includes the complete credential header set and exact multipart field names.
- Existing local, Codex ASR, and direct ElevenLabs behavior is unchanged.
- No real credential or recording is present in git, tests, logs, screenshots, or documentation.
- No permanent machine networking or trust-store changes are needed.
- `tag_audio_events`, diarization, realtime transcription, S1, chat, sync, and account operations are not presented as implemented unless separately captured and verified.

## Reference artifacts

Use `RESEARCH_FINDINGS.md` as the evidence record and `superwhisper-api.openapi.yaml` as the schema reference. Where they differ from an assumption in this plan, prefer a curl-verified or cache-confirmed fact from the findings.

Evidence confidence for the first provider:

```text
endpoint             curl-verified
explicit headers      curl-verified
multipart names       curl-verified
Ogg Opus upload       curl-verified
response shape        curl-verified
credential reuse      curl-verified for a new body/time/boundary
signature derivation  unknown and unnecessary for current reuse
batch audio-events    unverified
S1 token bootstrap    incomplete
```
