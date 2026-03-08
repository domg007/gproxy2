#!/usr/bin/env bash
set -euo pipefail

DEFAULT_PROMPT="explain what is AI in 100 words"

BASE_URL="${BASE_URL:-http://127.0.0.1:8787}"
API_KEY="${API_KEY:-}"
PROVIDER="${PROVIDER:-}"
METHOD="${METHOD:-}"
MODEL="${MODEL:-}"
PROMPT="${PROMPT:-$DEFAULT_PROMPT}"
TIMEOUT_SECONDS="${TIMEOUT_SECONDS:-120}"
VERBOSE="${VERBOSE:-0}"

usage() {
  cat <<'USAGE'
Usage:
  tests/provider/curl_provider.sh --provider <provider> --method <method> [--model <model>] [--prompt <text>] [--api-key <key>] [--base-url <url>] [--timeout <seconds>] [--verbose]

Positional shortcut:
  tests/provider/curl_provider.sh <provider> <method> [model]

Methods:
  model_list
  model_get
  openai_model_list
  openai_model_get
  claude_model_list
  claude_model_get
  gemini_model_list
  gemini_model_get
  openai_chat
  openai_chat_stream
  openai_responses
  openai_responses_stream
  openai_input_tokens
  openai_image_generate
  openai_image_generate_stream
  openai_image_edit
  openai_image_edit_stream
  openai_video_create
  openai_embeddings
  embeddings
  openai_compact
  compact
  claude_messages
  claude_messages_stream
  claude_count_tokens
  gemini_generate
  gemini_stream_generate
  gemini_stream_generate_sse
  gemini_stream_generate_ndjson
  gemini_count_tokens
  gemini_embeddings

Environment variables (optional):
  BASE_URL, API_KEY, PROVIDER, METHOD, MODEL, PROMPT, TIMEOUT_SECONDS, VERBOSE

Notes:
  - Default prompt: "explain what is AI in 100 words"
  - All requests use header: x-api-key
USAGE
}

json_escape() {
  local value="$1"
  value=${value//\\/\\\\}
  value=${value//\"/\\\"}
  value=${value//$'\n'/\\n}
  value=${value//$'\r'/\\r}
  value=${value//$'\t'/\\t}
  printf '%s' "$value"
}

normalize_gemini_model_path() {
  local value="$1"
  value="${value#/}"
  if [[ "$value" == models/* ]]; then
    printf '%s' "$value"
  else
    printf 'models/%s' "$value"
  fi
}

require_non_empty() {
  local name="$1"
  local value="$2"
  if [[ -z "${value// }" ]]; then
    echo "error: missing required value: $name" >&2
    exit 2
  fi
}

if [[ $# -gt 0 && "${1:-}" != "--help" && "${1:-}" != "-h" && "${1:0:2}" != "--" ]]; then
  PROVIDER="${1:-$PROVIDER}"
  METHOD="${2:-$METHOD}"
  MODEL="${3:-$MODEL}"
  shift $(( $# >= 3 ? 3 : $# ))
fi

while [[ $# -gt 0 ]]; do
  case "$1" in
    --provider)
      PROVIDER="$2"
      shift 2
      ;;
    --method)
      METHOD="$2"
      shift 2
      ;;
    --model)
      MODEL="$2"
      shift 2
      ;;
    --prompt)
      PROMPT="$2"
      shift 2
      ;;
    --api-key)
      API_KEY="$2"
      shift 2
      ;;
    --base-url)
      BASE_URL="$2"
      shift 2
      ;;
    --timeout)
      TIMEOUT_SECONDS="$2"
      shift 2
      ;;
    --verbose)
      VERBOSE=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "error: unknown argument: $1" >&2
      usage
      exit 2
      ;;
  esac
done

require_non_empty "provider" "$PROVIDER"
require_non_empty "method" "$METHOD"
require_non_empty "api-key (or API_KEY env)" "$API_KEY"

if ! [[ "$TIMEOUT_SECONDS" =~ ^[0-9]+$ ]]; then
  echo "error: --timeout must be an integer" >&2
  exit 2
fi

method_upper=""
path=""
body=""
extra_headers=()

escaped_prompt="$(json_escape "$PROMPT")"
tiny_png_data_url="data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO9Wv7sAAAAASUVORK5CYII="

case "$METHOD" in
  model_list)
    method_upper="GET"
    path="/$PROVIDER/v1/models"
    ;;
  model_get)
    require_non_empty "model" "$MODEL"
    method_upper="GET"
    path="/$PROVIDER/v1/models/$MODEL"
    ;;
  openai_model_list)
    method_upper="GET"
    path="/$PROVIDER/v1/models"
    ;;
  openai_model_get)
    require_non_empty "model" "$MODEL"
    method_upper="GET"
    path="/$PROVIDER/v1/models/$MODEL"
    ;;
  claude_model_list)
    method_upper="GET"
    path="/$PROVIDER/v1/models"
    ;;
  claude_model_get)
    require_non_empty "model" "$MODEL"
    method_upper="GET"
    path="/$PROVIDER/v1/models/$MODEL"
    ;;
  gemini_model_list)
    method_upper="GET"
    path="/$PROVIDER/v1beta/models"
    ;;
  gemini_model_get)
    require_non_empty "model" "$MODEL"
    method_upper="GET"
    gemini_model="$(normalize_gemini_model_path "$MODEL")"
    path="/$PROVIDER/v1beta/$gemini_model"
    ;;
  openai_chat)
    require_non_empty "model" "$MODEL"
    method_upper="POST"
    path="/$PROVIDER/v1/chat/completions"
    body_json="{\"model\":\"$(json_escape "$MODEL")\",\"messages\":[{\"role\":\"user\",\"content\":\"$escaped_prompt\"}],\"stream\":false}"
    body="$body_json"
    ;;
  openai_chat_stream)
    require_non_empty "model" "$MODEL"
    method_upper="POST"
    path="/$PROVIDER/v1/chat/completions"
    body_json="{\"model\":\"$(json_escape "$MODEL")\",\"messages\":[{\"role\":\"user\",\"content\":\"$escaped_prompt\"}],\"stream\":true}"
    body="$body_json"
    ;;
  openai_responses)
    require_non_empty "model" "$MODEL"
    method_upper="POST"
    path="/$PROVIDER/v1/responses"
    body_json="{\"model\":\"$(json_escape "$MODEL")\",\"input\":\"$escaped_prompt\",\"stream\":false}"
    body="$body_json"
    ;;
  openai_responses_stream)
    require_non_empty "model" "$MODEL"
    method_upper="POST"
    path="/$PROVIDER/v1/responses"
    body_json="{\"model\":\"$(json_escape "$MODEL")\",\"input\":\"$escaped_prompt\",\"stream\":true}"
    body="$body_json"
    ;;
  openai_input_tokens)
    require_non_empty "model" "$MODEL"
    method_upper="POST"
    path="/$PROVIDER/v1/responses/input_tokens"
    body_json="{\"model\":\"$(json_escape "$MODEL")\",\"input\":\"$escaped_prompt\"}"
    body="$body_json"
    ;;
  openai_image_generate)
    require_non_empty "model" "$MODEL"
    method_upper="POST"
    path="/$PROVIDER/v1/images/generations"
    body_json="{\"model\":\"$(json_escape "$MODEL")\",\"prompt\":\"$escaped_prompt\",\"size\":\"1024x1024\",\"stream\":false}"
    body="$body_json"
    ;;
  openai_image_generate_stream)
    require_non_empty "model" "$MODEL"
    method_upper="POST"
    path="/$PROVIDER/v1/images/generations"
    body_json="{\"model\":\"$(json_escape "$MODEL")\",\"prompt\":\"$escaped_prompt\",\"size\":\"1024x1024\",\"stream\":true}"
    body="$body_json"
    ;;
  openai_image_edit)
    require_non_empty "model" "$MODEL"
    method_upper="POST"
    path="/$PROVIDER/v1/images/edits"
    body_json="{\"model\":\"$(json_escape "$MODEL")\",\"prompt\":\"$escaped_prompt\",\"images\":[{\"image_url\":\"$tiny_png_data_url\"}],\"size\":\"1024x1024\",\"stream\":false}"
    body="$body_json"
    ;;
  openai_image_edit_stream)
    require_non_empty "model" "$MODEL"
    method_upper="POST"
    path="/$PROVIDER/v1/images/edits"
    body_json="{\"model\":\"$(json_escape "$MODEL")\",\"prompt\":\"$escaped_prompt\",\"images\":[{\"image_url\":\"$tiny_png_data_url\"}],\"size\":\"1024x1024\",\"stream\":true}"
    body="$body_json"
    ;;
  openai_video_create)
    require_non_empty "model" "$MODEL"
    method_upper="POST"
    path="/$PROVIDER/v1/videos"
    body_json="{\"model\":\"$(json_escape "$MODEL")\",\"prompt\":\"$escaped_prompt\",\"seconds\":\"8\",\"size\":\"1280x720\"}"
    body="$body_json"
    ;;
  openai_embeddings|embeddings)
    require_non_empty "model" "$MODEL"
    method_upper="POST"
    path="/$PROVIDER/v1/embeddings"
    body_json="{\"model\":\"$(json_escape "$MODEL")\",\"input\":\"$escaped_prompt\"}"
    body="$body_json"
    ;;
  openai_compact|compact)
    require_non_empty "model" "$MODEL"
    method_upper="POST"
    path="/$PROVIDER/v1/responses/compact"
    body_json="{\"model\":\"$(json_escape "$MODEL")\",\"instructions\":\"$escaped_prompt\",\"input\":\"$escaped_prompt\"}"
    body="$body_json"
    ;;
  claude_messages)
    require_non_empty "model" "$MODEL"
    method_upper="POST"
    path="/$PROVIDER/v1/messages"
    body_json="{\"model\":\"$(json_escape "$MODEL")\",\"max_tokens\":256,\"messages\":[{\"role\":\"user\",\"content\":\"$escaped_prompt\"}],\"stream\":false}"
    body="$body_json"
    extra_headers+=( "anthropic-version: 2023-06-01" )
    ;;
  claude_messages_stream)
    require_non_empty "model" "$MODEL"
    method_upper="POST"
    path="/$PROVIDER/v1/messages"
    body_json="{\"model\":\"$(json_escape "$MODEL")\",\"max_tokens\":256,\"messages\":[{\"role\":\"user\",\"content\":\"$escaped_prompt\"}],\"stream\":true}"
    body="$body_json"
    extra_headers+=( "anthropic-version: 2023-06-01" )
    ;;
  claude_count_tokens)
    require_non_empty "model" "$MODEL"
    method_upper="POST"
    path="/$PROVIDER/v1/messages/count_tokens"
    body_json="{\"model\":\"$(json_escape "$MODEL")\",\"messages\":[{\"role\":\"user\",\"content\":\"$escaped_prompt\"}]}"
    body="$body_json"
    extra_headers+=( "anthropic-version: 2023-06-01" )
    ;;
  gemini_generate)
    require_non_empty "model" "$MODEL"
    method_upper="POST"
    gemini_model="$(normalize_gemini_model_path "$MODEL")"
    path="/$PROVIDER/v1beta/$gemini_model:generateContent"
    body="{\"contents\":[{\"role\":\"user\",\"parts\":[{\"text\":\"$escaped_prompt\"}]}]}"
    ;;
  gemini_stream_generate|gemini_stream_generate_sse)
    require_non_empty "model" "$MODEL"
    method_upper="POST"
    gemini_model="$(normalize_gemini_model_path "$MODEL")"
    path="/$PROVIDER/v1beta/$gemini_model:streamGenerateContent?alt=sse"
    body="{\"contents\":[{\"role\":\"user\",\"parts\":[{\"text\":\"$escaped_prompt\"}]}]}"
    ;;
  gemini_stream_generate_ndjson)
    require_non_empty "model" "$MODEL"
    method_upper="POST"
    gemini_model="$(normalize_gemini_model_path "$MODEL")"
    path="/$PROVIDER/v1beta/$gemini_model:streamGenerateContent"
    body="{\"contents\":[{\"role\":\"user\",\"parts\":[{\"text\":\"$escaped_prompt\"}]}]}"
    ;;
  gemini_count_tokens)
    require_non_empty "model" "$MODEL"
    method_upper="POST"
    gemini_model="$(normalize_gemini_model_path "$MODEL")"
    path="/$PROVIDER/v1beta/$gemini_model:countTokens"
    body="{\"contents\":[{\"role\":\"user\",\"parts\":[{\"text\":\"$escaped_prompt\"}]}]}"
    ;;
  gemini_embeddings)
    require_non_empty "model" "$MODEL"
    method_upper="POST"
    gemini_model="$(normalize_gemini_model_path "$MODEL")"
    path="/$PROVIDER/v1beta/$gemini_model:embedContent"
    body="{\"content\":{\"parts\":[{\"text\":\"$escaped_prompt\"}]}}"
    ;;
  *)
    echo "error: unsupported method: $METHOD" >&2
    usage
    exit 2
    ;;
esac

url="${BASE_URL%/}$path"

tmp_body="$(mktemp)"
cleanup() {
  rm -f "$tmp_body"
}
trap cleanup EXIT

curl_args=(
  -sS
  -X "$method_upper"
  --max-time "$TIMEOUT_SECONDS"
  -H "x-api-key: $API_KEY"
)

if [[ "$BASE_URL" == http://127.0.0.1* || "$BASE_URL" == https://127.0.0.1* || "$BASE_URL" == http://localhost* || "$BASE_URL" == https://localhost* ]]; then
  curl_args+=( --noproxy "*" )
fi

for h in "${extra_headers[@]}"; do
  curl_args+=( -H "$h" )
done

if [[ -n "$body" ]]; then
  curl_args+=( -H "content-type: application/json" --data "$body" )
fi

if [[ "$VERBOSE" == "1" ]]; then
  set -x
fi

status_code="$(curl "${curl_args[@]}" -o "$tmp_body" -w "%{http_code}" "$url")"

if [[ "$VERBOSE" == "1" ]]; then
  set +x
fi

echo "[request] provider=$PROVIDER method=$METHOD model=${MODEL:-<none>}"
echo "[request] $method_upper $url"
echo "[response] status=$status_code"
cat "$tmp_body"

if [[ ! "$status_code" =~ ^[0-9]+$ ]]; then
  echo "\nerror: invalid status code: $status_code" >&2
  exit 1
fi

if (( status_code >= 400 )); then
  echo "\nerror: upstream/provider request failed with status $status_code" >&2
  exit 1
fi
