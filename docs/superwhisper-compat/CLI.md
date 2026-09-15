# Standalone Bun CLI

`scripts/superwhisper.ts` runs independently of Handy with no package dependencies. It needs Bun, ffmpeg, and ffprobe on PATH. Copying that single script elsewhere is sufficient.

## Usage

The CLI uses `SW_X_ID`, `SW_X_LICENSE`, and `SW_X_SIGNATURE` from the environment first. For each missing or blank value, it reads `settings.transcription_api_keys` from Handy's existing settings file. If all three environment values are present, it does not read the file. Handy settings are never modified.

Default settings paths:

- macOS: `~/Library/Application Support/com.pais.handy/settings_store.json`
- Linux: `$XDG_DATA_HOME/com.pais.handy/settings_store.json`, defaulting to `~/.local/share/com.pais.handy/settings_store.json`
- Windows: `%APPDATA%/com.pais.handy/settings_store.json`

Use `--handy-config /path/to/settings_store.json` for a portable installation or a custom location. A missing file is allowed if credentials can otherwise be resolved. Unreadable or malformed settings produce an error when the file is needed. Offline `--from-json` rendering needs neither credentials nor Handy settings.

If Superwhisper is already configured in Handy, run directly:

```bash
bun scripts/superwhisper.ts movie.mp4 --subtitles --diarize
```

You can still use a private environment file:

```bash
bun --env-file="$HOME/.config/superwhisper-replay/env" scripts/superwhisper.ts recording.m4a

bun --env-file="$HOME/.config/superwhisper-replay/env" scripts/superwhisper.ts \
  interview.mp4 --subtitles --diarize --tag-audio-events \
  --output interview.srt --save-json interview.json

bun scripts/superwhisper.ts --help
```

The default model is `scribe`, ElevenLabs Scribe through Superwhisper. The proxy chooses the revision; `--model` cannot pin `scribe_v1` or `scribe_v2`. `--model s1-voice` uses Superwhisper's regional S1 service for plain transcription. This CLI requires Scribe for subtitles, diarization, and audio-event options.

Plain text goes to stdout by default. `--subtitles movie.mp4` creates `movie.srt` beside the input so video players can discover it. `--output -` sends subtitles to stdout. `--output` and `--save-json` create new files with owner-only permissions and refuse existing files. Use both flags to retain the formatted transcript and the original response. The JSON file is saved even if subtitle rendering fails because the response lacks timestamps or speaker labels.

`--diarize` requests speaker separation and preserves returned IDs, for example `speaker_0:` and `speaker_1:`. Find and replace `speaker_0:` with `Adin:` in the text or SRT file after listening to identify that speaker. IDs apply to a single recording; they do not identify the same person across recordings. Audio events without a speaker ID remain unlabeled.

`--tag-audio-events` requests sound tags. `--no-tag-audio-events` explicitly disables them. Omitting both leaves Superwhisper's default in effect. `--language en` supplies a language hint; otherwise the server detects the language.

FFprobe selects the first audio track from any input FFmpeg can read. Opus, MP3, AAC, FLAC, and 16-bit PCM are copied into Ogg, MP3, M4A, FLAC, or WAV containers without re-encoding. Other codecs are decoded to 16-bit PCM WAV without resampling or downmixing. This avoids another lossy encoding step, but WAV uploads can be much larger. Extraction uses a temporary directory. A delayed audio track's start offset is retained in saved JSON and added to subtitle timestamps. Multiple audio tracks are not merged. `--no-convert` sends the original file with its filename and Bun's inferred MIME type, without requiring ffmpeg. The server must support that input format. Requests upload audio and may consume account usage. The CLI does not retry failed requests automatically.

## Reformat without another upload

```bash
bun scripts/superwhisper.ts interview.json --from-json --diarize --output interview.txt
bun scripts/superwhisper.ts interview.json --from-json --subtitles --diarize --output revised.srt
```

Offline rendering uses the saved response as-is. It cannot add diarization or audio events that the original request did not return. SRT cues split at speaker changes, sentence endings, pauses over 0.8 seconds, roughly 80 text characters, or six seconds. Individual words and audio-event labels remain intact. Missing timestamps cause an error instead of fabricated timing.

## Verification

```bash
bun test scripts/superwhisper.test.ts
```

The local tests cover multipart options, subtitle rendering, speaker labels, offline execution, and output preservation. The existing proxy research confirms Scribe word timestamps and audio-event tagging. Proxy diarization is recorded as an optional request field but still needs a live recording to verify its response. ElevenLabs' [direct API documentation](https://elevenlabs.io/docs/api-reference/speech-to-text/convert) describes `diarize` and `speaker_id`; it does not establish proxy compatibility.
