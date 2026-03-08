#!/usr/bin/env bash
set -euo pipefail

BASE_URL="${BASE_URL:-http://127.0.0.1:8787}"
API_KEY="${API_KEY:-}"
PROVIDER="${PROVIDER:-}"
MODEL="${MODEL:-}"
EMBEDDING_MODEL="${EMBEDDING_MODEL:-}"
GEMINI_EMBEDDING_MODEL="${GEMINI_EMBEDDING_MODEL:-}"
IMAGE_MODEL="${IMAGE_MODEL:-}"
IMAGE_EDIT_MODEL="${IMAGE_EDIT_MODEL:-}"
VIDEO_MODEL="${VIDEO_MODEL:-}"
TIMEOUT_SECONDS="${TIMEOUT_SECONDS:-120}"
RERUN="${RERUN:-0}"

RESULT_DIR="tests/result"
LOG_DIR="tests/provider/logs"

usage() {
  cat <<'USAGE'
Usage:
  tests/provider/run_channel_regression.sh \
    --provider <provider> \
    --model <model> \
    [--embedding-model <embedding-model>] \
    [--gemini-embedding-model <gemini-embedding-model>] \
    [--image-model <image-model>] \
    [--image-edit-model <image-edit-model>] \
    [--video-model <video-model>] \
    [--api-key <key>] \
    [--base-url <url>] \
    [--timeout <seconds>] \
    [--rerun]

Behavior:
  - Writes per-provider progress to tests/result/<provider>.md
  - Skips methods that already have PASS for the same model (unless --rerun)
USAGE
}

require_non_empty() {
  local name="$1"
  local value="$2"
  if [[ -z "${value// }" ]]; then
    echo "error: missing required value: $name" >&2
    exit 2
  fi
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --provider)
      PROVIDER="$2"
      shift 2
      ;;
    --model)
      MODEL="$2"
      shift 2
      ;;
    --embedding-model)
      EMBEDDING_MODEL="$2"
      shift 2
      ;;
    --gemini-embedding-model)
      GEMINI_EMBEDDING_MODEL="$2"
      shift 2
      ;;
    --image-model)
      IMAGE_MODEL="$2"
      shift 2
      ;;
    --image-edit-model)
      IMAGE_EDIT_MODEL="$2"
      shift 2
      ;;
    --video-model)
      VIDEO_MODEL="$2"
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
    --rerun)
      RERUN=1
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
require_non_empty "model" "$MODEL"
require_non_empty "api-key (or API_KEY env)" "$API_KEY"

mkdir -p "$RESULT_DIR" "$LOG_DIR"

RESULT_FILE="$RESULT_DIR/${PROVIDER}.md"

init_result_file() {
  if [[ -f "$RESULT_FILE" ]]; then
    return
  fi

  cat > "$RESULT_FILE" <<EOF
# Provider Regression Result: ${PROVIDER}

| Time (UTC) | Method | Model | Status | HTTP | Detail | Log |
|---|---|---|---|---|---|---|
EOF
}

append_result() {
  local method="$1"
  local model="$2"
  local status="$3"
  local http="$4"
  local detail="$5"
  local log_path="$6"
  local ts
  ts="$(date -u '+%Y-%m-%d %H:%M:%S')"
  detail="${detail//|//}"
  printf '| %s | %s | %s | %s | %s | %s | `%s` |\n' \
    "$ts" "$method" "$model" "$status" "$http" "$detail" "$log_path" >> "$RESULT_FILE"
}

already_passed() {
  local method="$1"
  local model="$2"
  rg -q "\\| ${method} \\| ${model} \\| PASS \\|" "$RESULT_FILE"
}

run_one() {
  local method="$1"
  local model="$2"

  if [[ "$RERUN" != "1" ]] && already_passed "$method" "$model"; then
    echo "SKIP $PROVIDER $method $model (already PASS)"
    return
  fi

  local safe_model
  safe_model="$(echo "$model" | tr '/ ' '__')"
  local log_path="$LOG_DIR/${PROVIDER}__${method}__${safe_model}.log"

  local cmd=(tests/provider/curl_provider.sh --provider "$PROVIDER" --method "$method" --api-key "$API_KEY" --base-url "$BASE_URL" --timeout "$TIMEOUT_SECONDS")
  case "$method" in
    openai_model_list|claude_model_list|gemini_model_list)
      ;;
    *)
      cmd+=(--model "$model")
      ;;
  esac

  local output rc http detail
  set +e
  output="$("${cmd[@]}" 2>&1)"
  rc=$?
  set -e
  printf '%s\n' "$output" > "$log_path"

  http="$(printf '%s\n' "$output" | rg -o "\\[response\\] status=[0-9]+" -N | tail -n1 | awk -F= '{print $2}')"
  if [[ -z "$http" ]]; then
    http="n/a"
  fi

  if [[ $rc -eq 0 ]]; then
    append_result "$method" "$model" "PASS" "$http" "ok" "$log_path"
    echo "PASS $PROVIDER $method $model (http=$http)"
    return
  fi

  detail="$(printf '%s\n' "$output" | tail -n 1)"
  append_result "$method" "$model" "FAIL" "$http" "${detail:0:180}" "$log_path"
  echo "FAIL $PROVIDER $method $model (http=$http)"
}

init_result_file

methods=(
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
  openai_compact
  claude_messages
  claude_messages_stream
  claude_count_tokens
  gemini_generate
  gemini_stream_generate_sse
  gemini_stream_generate_ndjson
  gemini_count_tokens
)

for method in "${methods[@]}"; do
  run_one "$method" "$MODEL"
done

if [[ -n "${EMBEDDING_MODEL// }" ]]; then
  run_one "openai_embeddings" "$EMBEDDING_MODEL"
fi

if [[ -n "${GEMINI_EMBEDDING_MODEL// }" ]]; then
  run_one "gemini_embeddings" "$GEMINI_EMBEDDING_MODEL"
fi

if [[ -n "${IMAGE_MODEL// }" ]]; then
  run_one "openai_image_generate" "$IMAGE_MODEL"
  run_one "openai_image_generate_stream" "$IMAGE_MODEL"
fi

if [[ -n "${IMAGE_EDIT_MODEL// }" ]]; then
  run_one "openai_image_edit" "$IMAGE_EDIT_MODEL"
  run_one "openai_image_edit_stream" "$IMAGE_EDIT_MODEL"
fi

if [[ -n "${VIDEO_MODEL// }" ]]; then
  run_one "openai_video_create" "$VIDEO_MODEL"
fi
