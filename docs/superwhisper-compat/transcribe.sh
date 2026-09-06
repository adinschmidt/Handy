#!/usr/bin/env bash
#
# Transcribe an audio file with Scribe and print the transcript with
# word/segment timestamps.
#
#   ./transcribe.sh recording.m4a
#   ./transcribe.sh --format srt --language en interview.wav > interview.srt
#   ./transcribe.sh --route direct recording.m4a     # pin scribe_v2 explicitly
#
# Two routes, because the model is selected by *which endpoint you call*:
#
#   proxy  (default)  POST api.superwhisper.com/elevenlabs/v1/transcribe
#                     The route itself tells Superwhisper's backend to use its
#                     configured ElevenLabs transcription service; the backend
#                     pins the Scribe revision. There is no version selector in
#                     the request, so the client cannot guarantee which
#                     revision runs -- currently advertised as Scribe V2.
#                     Credentials (see RESEARCH_FINDINGS.md):
#                       export SW_X_ID='<device UUID>'
#                       export SW_X_LICENSE='<license UUID>'
#                       export SW_X_SIGNATURE='<64-char signature>'
#
#   direct            POST api.elevenlabs.io/v1/speech-to-text
#                     ElevenLabs' own batch API, which requires an explicit
#                     model_id. Use this when you need explicit model
#                     selection rather than a server-chosen revision.
#                     ElevenLabs may still revise or retire a model id. Not
#                     part of the captured traffic -- this is ElevenLabs'
#                     public API, and it bills your own account.
#                       export ELEVENLABS_API_KEY='<xi-api-key>'
#
# Credentials are read from the environment and are never written to disk by
# this script. Alternatively put the exports in
# ~/.config/superwhisper-replay/env (or the file named by $SW_ENV_FILE) and
# this script will source it.

set -euo pipefail

PROXY_URL="${SW_API_URL:-https://api.superwhisper.com/elevenlabs/v1/transcribe}"
DIRECT_URL="${ELEVENLABS_API_URL:-https://api.elevenlabs.io/v1/speech-to-text}"
USER_AGENT='superwhisper/2.16.6 (com.superduper.superwhisper; build:2.16.6; macOS 26.5.2) Alamofire/5.8.0'

route='proxy'
# Every optional part is omitted unless asked for, so the default proxy request
# carries only `file`. An empty language means server-side auto-detection.
# Note: the OpenAPI schema derived from the capture lists language_code as
# required; if a bare request is rejected, pass -l en.
language="${SW_LANGUAGE_CODE-}"
# Only meaningful on the direct route; ElevenLabs requires it there. The proxy
# route was never observed carrying a model selector and does not need one.
model="${ELEVENLABS_MODEL_ID:-scribe_v2}"
format='timestamps'
diarize=''
output=''
save_json=''
convert=1
keyterms=()

die() { printf 'transcribe.sh: %s\n' "$1" >&2; exit 1; }

usage() {
  cat <<'EOF'
Usage: transcribe.sh [options] <audio-file>

Options:
  -r, --route ROUTE     Which transcription path to call (default: proxy)
                          proxy   Superwhisper's ElevenLabs route; the backend
                                  picks the Scribe revision (currently V2)
                          direct  ElevenLabs' own API with an explicit
                                  model_id, billed to your own account
  -l, --language CODE   Language code (default: omitted, server auto-detects)
  -m, --model ID        model_id for --route direct (default: scribe_v2)
  -f, --format FORMAT   Output format (default: timestamps)
                          timestamps  [mm:ss.mmm -> mm:ss.mmm] segment text
                          words       one word per line with start/end
                          text        plain transcript, no timestamps
                          srt         SubRip subtitles
                          vtt         WebVTT subtitles
                          json        raw API response
  -k, --keyterm TERM    Vocabulary hint; repeatable
  -d, --diarize         Ask for speaker separation. On the proxy route this
                        field is binary-confirmed but was not in the captured
                        request, so it is untested against the live service.
  -o, --output FILE     Write to FILE instead of stdout
      --save-json FILE  Also save the raw JSON response to FILE
      --no-convert      Upload the file as-is instead of transcoding to Ogg Opus
  -h, --help            Show this help

Environment:
  SW_X_ID, SW_X_LICENSE, SW_X_SIGNATURE   credentials for --route proxy
  ELEVENLABS_API_KEY                      credential for --route direct
  SW_ENV_FILE                             file to source them from
                                          (default: ~/.config/superwhisper-replay/env)
EOF
}

while [ $# -gt 0 ]; do
  case "$1" in
    -r|--route)     route="${2-}"; shift 2 ;;
    -l|--language)  language="${2-}"; shift 2 ;;
    -m|--model)     model="${2-}"; shift 2 ;;
    -f|--format)    format="${2-}"; shift 2 ;;
    -k|--keyterm)   keyterms+=("${2-}"); shift 2 ;;
    -o|--output)    output="${2-}"; shift 2 ;;
    --save-json)    save_json="${2-}"; shift 2 ;;
    -d|--diarize)   diarize=1; shift ;;
    --no-convert)   convert=''; shift ;;
    -h|--help)      usage; exit 0 ;;
    --)             shift; break ;;
    -*)             die "unknown option: $1" ;;
    *)              break ;;
  esac
done

[ $# -ge 1 ] || { usage >&2; exit 2; }
[ $# -eq 1 ] || die "expected exactly one audio file, got $#"
input="$1"
[ -f "$input" ] || die "no such file: $input"

case "$format" in
  timestamps|words|text|srt|vtt|json) ;;
  *) die "unknown format: $format" ;;
esac

case "$route" in
  proxy|direct) ;;
  *) die "unknown route: $route (expected proxy or direct)" ;;
esac

env_file="${SW_ENV_FILE:-$HOME/.config/superwhisper-replay/env}"
if [ -f "$env_file" ]; then
  # shellcheck disable=SC1090
  . "$env_file"
fi

if [ "$route" = 'proxy' ]; then
  required=(SW_X_ID SW_X_LICENSE SW_X_SIGNATURE)
else
  required=(ELEVENLABS_API_KEY)
  [ -n "$model" ] || die "--route direct requires a model id (--model scribe_v2)"
fi
for var in "${required[@]}"; do
  [ -n "${!var-}" ] || die "missing required environment variable: $var (see the header of this script)"
done

for cmd in curl jq; do
  command -v "$cmd" >/dev/null 2>&1 || die "required command not found: $cmd"
done

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

# The app uploads mono 16 kHz Ogg Opus; transcode anything else to match.
upload="$input"
if [ -n "$convert" ]; then
  command -v ffmpeg >/dev/null 2>&1 || die "ffmpeg not found (use --no-convert to upload the file as-is)"
  upload="$tmpdir/audio.ogg"
  ffmpeg -hide_banner -loglevel error -nostdin \
    -i "$input" -vn -ar 16000 -ac 1 -c:a libopus -b:a 32k "$upload" \
    || die "ffmpeg failed to convert $input"
fi

args=(
  --http2 --compressed --silent --show-error
  --request POST
  --write-out '\n%{http_code}'
  --form "file=@${upload};type=audio/ogg;filename=audio.ogg"
)
[ -n "$language" ] && args+=(--form "language_code=${language}")

if [ "$route" = 'proxy' ]; then
  # Capture-confirmed parts: file, language_code, keyterms[].
  # Binary-confirmed optional part: diarize.
  # No model part -- the route selects the backend, and the captured request
  # carried no version selector.
  args+=(
    "$PROXY_URL"
    --header 'Accept: */*'
    --header 'Accept-Language: en-CA,en-US;q=0.9,en;q=0.8'
    --header "User-Agent: $USER_AGENT"
    --header "X-ID: $SW_X_ID"
    --header "X-License: $SW_X_LICENSE"
    --header 'X-Platform: macos'
    --header "X-Signature: $SW_X_SIGNATURE"
  )
else
  args+=(
    "$DIRECT_URL"
    --header "xi-api-key: $ELEVENLABS_API_KEY"
    --form "model_id=${model}"
    --form 'timestamps_granularity=word'
  )
fi

[ -n "$diarize" ] && args+=(--form 'diarize=true')
for term in ${keyterms[@]+"${keyterms[@]}"}; do
  args+=(--form "keyterms[]=${term}")
done

response="$(curl "${args[@]}")" || die "request failed"
status="${response##*$'\n'}"
body="${response%$'\n'*}"

if [ "$status" != '200' ]; then
  printf 'transcribe.sh: HTTP %s\n%s\n' "$status" "$body" >&2
  exit 1
fi

[ -n "$save_json" ] && printf '%s\n' "$body" > "$save_json"

# Segment words into readable lines: break on a sentence-final word, on a pause
# longer than GAP seconds, or once a segment reaches MAXDUR seconds.
read -r -d '' jq_lib <<'JQ' || true
def pad($n): tostring | ("0" * $n + .)[-$n:];
def clock($t; $sep):
  (($t // 0) * 1000 | round) as $ms
  | (($ms / 1000) | floor) as $s
  | "\(($s / 3600 | floor) | pad(2)):\(($s % 3600 / 60 | floor) | pad(2)):\(($s % 60) | pad(2))\($sep)\(($ms % 1000) | pad(3))";
# Normalize the entries both routes return: keep words and audio events, drop
# spacing, tolerate a missing type/start/end and any extra fields (speaker_id,
# channel_index, logprob, whatever gets added later).
def spoken:
  [ .words // [] | .[]
    | select((.type // "word") != "spacing")
    | {start: (.start // 0), end: (.end // .start // 0),
       text: (.text // ""), speaker: .speaker_id}
    | select(.text != "") ];
def segments($gap; $maxdur):
  reduce spoken[] as $w ([];
    if length == 0 then [$w]
    else .[-1] as $last
      | if ($w.start - $last.end > $gap)
           or ($w.end - $last.start > $maxdur)
           or ($w.speaker != $last.speaker)
           or ($last.text | test("[.!?…][\"')\\]]?$"))
        then . + [$w]
        else .[0:-1] + [$last + {end: $w.end, text: ($last.text + " " + $w.text)}]
        end
    end);
def speaker_prefix: if .speaker then "\(.speaker): " else "" end;
JQ

case "$format" in
  json)
    printf '%s\n' "$body" | jq . ;;
  text)
    printf '%s\n' "$body" | jq -r "$jq_lib"'
      .text // (spoken | map(.text) | join(" "))' ;;
  words)
    printf '%s\n' "$body" | jq -r "$jq_lib"'
      spoken[] | "[\(clock(.start; ".")) -> \(clock(.end; "."))] \(speaker_prefix)\(.text)"' ;;
  timestamps)
    printf '%s\n' "$body" | jq -r "$jq_lib"'
      segments(0.8; 12)[]
      | "[\(clock(.start; ".")) -> \(clock(.end; "."))] \(speaker_prefix)\(.text)"' ;;
  srt)
    printf '%s\n' "$body" | jq -r "$jq_lib"'
      segments(0.8; 6) | to_entries[]
      | "\(.key + 1)\n\(clock(.value.start; ",")) --> \(clock(.value.end; ","))\n\(.value | speaker_prefix)\(.value.text)\n"' ;;
  vtt)
    printf '%s\n' "$body" | jq -r "$jq_lib"'
      "WEBVTT\n", (segments(0.8; 6)[]
      | "\(clock(.start; ".")) --> \(clock(.end; "."))\n\(speaker_prefix)\(.text)\n")' ;;
esac > "${output:-/dev/stdout}"
