#!/bin/sh
#
# Superwhisper replay templates.
#
# This file does not contain credentials and does not make a request when run.
# Source it, set the documented environment variables, then invoke one function.

set -eu

sw_require_var() {
  sw_var_name="$1"
  eval "sw_var_value=\${$sw_var_name-}"
  if [ -z "$sw_var_value" ]; then
    echo "Missing required environment variable: $sw_var_name" >&2
    return 1
  fi
}

sw_transcribe_elevenlabs() {
  sw_audio_path="$1"
  sw_language_code="${2:-en}"

  sw_require_var SW_X_ID
  sw_require_var SW_X_LICENSE
  sw_require_var SW_X_SIGNATURE

  curl --http2 --fail-with-body --compressed \
    --request POST \
    'https://api.superwhisper.com/elevenlabs/v1/transcribe' \
    --header 'Accept: */*' \
    --header 'Accept-Encoding: gzip, deflate' \
    --header 'Accept-Language: en-CA,en-US;q=0.9,en;q=0.8' \
    --header 'User-Agent: superwhisper/2.16.6 (com.superduper.superwhisper; build:2.16.6; macOS 26.5.2) Alamofire/5.8.0' \
    --header "X-ID: ${SW_X_ID}" \
    --header "X-License: ${SW_X_LICENSE}" \
    --header 'X-Platform: macos' \
    --header "X-Signature: ${SW_X_SIGNATURE}" \
    --form "file=@${sw_audio_path};type=audio/ogg;filename=audio.ogg" \
    --form "language_code=${sw_language_code}"
}

sw_transcribe_deepgram() {
  sw_audio_path="$1"
  sw_language_code="${2:-en}"
  sw_model="${3:-nova-3}"

  sw_require_var DEEPGRAM_TOKEN

  curl --fail-with-body \
    --request POST \
    'https://api.deepgram.com/v1/listen' \
    --url-query 'punctuate=true' \
    --url-query 'paragraphs=true' \
    --url-query 'numerals=true' \
    --url-query 'smart_format=true' \
    --url-query "language=${sw_language_code}" \
    --url-query "model=${sw_model}" \
    --header "Authorization: Token ${DEEPGRAM_TOKEN}" \
    --header 'Content-Type: audio/ogg' \
    --data-binary "@${sw_audio_path}"
}

sw_transcribe_openai() {
  sw_audio_path="$1"
  sw_language_code="${2:-en}"
  sw_model="${3:-whisper-1}"

  sw_require_var OPENAI_API_KEY

  curl --fail-with-body \
    --request POST \
    'https://api.openai.com/v1/audio/transcriptions' \
    --header "Authorization: Bearer ${OPENAI_API_KEY}" \
    --form "file=@${sw_audio_path};type=audio/ogg" \
    --form "model=${sw_model}" \
    --form "language=${sw_language_code}" \
    --form 'response_format=verbose_json' \
    --form 'timestamp_granularities[0]=word'
}

sw_chat_openai_format() {
  sw_request_json="$1"
  sw_path="${2:-/v1/chat/completions}"

  sw_require_var SW_X_ID
  sw_require_var SW_X_LICENSE
  sw_require_var SW_X_SIGNATURE

  curl --http2 --fail-with-body --no-buffer \
    --request POST \
    "https://api.superwhisper.com${sw_path}" \
    --header "X-ID: ${SW_X_ID}" \
    --header "X-License: ${SW_X_LICENSE}" \
    --header 'X-Platform: macos' \
    --header "X-Signature: ${SW_X_SIGNATURE}" \
    --header 'Content-Type: application/json' \
    --data-binary "@${sw_request_json}"
}

sw_s1_run() {
  sw_request_json="$1"

  sw_require_var SW_INFERENCE_JWT

  curl --fail-with-body \
    --request POST \
    'https://ai.superwhisper.com/v1/c/run' \
    --header "Authorization: Bearer ${SW_INFERENCE_JWT}" \
    --header 'Content-Type: application/json' \
    --data-binary "@${sw_request_json}"
}

echo "Replay functions loaded. No request was sent." >&2
