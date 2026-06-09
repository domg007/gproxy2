#!/usr/bin/env bash
# M1 smoke test: proves the passthrough pipeline forwards a real request.
# Boots gproxy (file backend, seeded) against a mock upstream and asserts the
# non-stream, streaming, auth-failure, and scoped-bypass paths.
set -u

HITLOG="$(mktemp)"
DATA_DIR="$(mktemp -d)"
MOCK_PY="$(mktemp --suffix=.py)"
PORT=8787
MOCK_PORT=9009
MOCK="" ; GP=""

cleanup() {
  [ -n "$GP" ] && kill "$GP" 2>/dev/null
  [ -n "$MOCK" ] && kill "$MOCK" 2>/dev/null
  rm -f "$HITLOG" "$MOCK_PY"
  rm -rf "$DATA_DIR"
}
trap cleanup EXIT

fail() { echo "FAIL: $*"; exit 1; }

cat > "$MOCK_PY" <<PY
from http.server import BaseHTTPRequestHandler, HTTPServer
import json, os
HITLOG = os.environ["HITLOG"]
class H(BaseHTTPRequestHandler):
    def do_POST(self):
        with open(HITLOG, "a") as f: f.write(self.path + "\n")
        n = int(self.headers.get('content-length', 0))
        body = json.loads(self.rfile.read(n) or b'{}')
        assert self.headers.get('Authorization') == 'Bearer sk-mock', \
            "upstream saw %r" % self.headers.get('Authorization')
        if body.get('stream'):
            self.send_response(200)
            self.send_header('content-type', 'text/event-stream'); self.end_headers()
            self.wfile.write(b'data: {"choices":[{"delta":{"content":"hi"}}]}\n\n')
            self.wfile.write(b'data: [DONE]\n\n'); self.wfile.flush()
        else:
            out = json.dumps({"id":"cmpl-1","object":"chat.completion",
                "choices":[{"message":{"role":"assistant","content":"pong"}}]}).encode()
            self.send_response(200)
            self.send_header('content-type','application/json'); self.end_headers()
            self.wfile.write(out)
    def log_message(self, *a): pass
HTTPServer(('127.0.0.1', ${MOCK_PORT}), H).serve_forever()
PY

HITLOG="$HITLOG" python3 "$MOCK_PY" & MOCK=$!

# Launch gproxy with proxy env stripped so wreq's upstream call to the local mock
# is direct (the dev box has http(s)_proxy set, which would otherwise be used).
env -u http_proxy -u https_proxy -u HTTP_PROXY -u HTTPS_PROXY -u ALL_PROXY -u all_proxy \
  GPROXY_SEED=1 GPROXY_DATA_DIR="$DATA_DIR" GPROXY_PORT=$PORT \
  cargo run -q --bin gproxy >/dev/null 2>&1 & GP=$!

# All curls bypass any ambient proxy (localhost must be direct).
CURL=(curl --noproxy '*')

for _ in $(seq 1 100); do
  "${CURL[@]}" -fsS "http://127.0.0.1:$PORT/healthz" >/dev/null 2>&1 && break
  sleep 0.3
done

# 1. non-stream aggregated
out=$("${CURL[@]}" -sS "http://127.0.0.1:$PORT/v1/chat/completions" \
  -H 'Authorization: Bearer sk-smoke-123' -H 'content-type: application/json' \
  -d '{"model":"gpt-4o-mini","messages":[{"role":"user","content":"ping"}]}')
echo "$out" | grep -qE '"content":[[:space:]]*"pong"' || fail "step1 non-stream: $out"
echo "PASS step1: non-stream aggregated"

# 2. streaming passthrough — exactly 2 data: frames
n=$("${CURL[@]}" -sS -N "http://127.0.0.1:$PORT/v1/chat/completions" \
  -H 'Authorization: Bearer sk-smoke-123' -H 'content-type: application/json' \
  -d '{"model":"gpt-4o-mini","stream":true,"messages":[{"role":"user","content":"ping"}]}' \
  | grep -c '^data:')
[ "$n" = "2" ] || fail "step2 stream: expected 2 data frames, got $n"
echo "PASS step2: streaming passthrough (2 frames)"

# 3. auth failure — 401 and ZERO upstream hits
: > "$HITLOG"
code=$("${CURL[@]}" -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$PORT/v1/chat/completions" \
  -H 'Authorization: Bearer wrong' -H 'content-type: application/json' \
  -d '{"model":"gpt-4o-mini","messages":[]}')
[ "$code" = "401" ] || fail "step3 auth: expected 401, got $code"
[ ! -s "$HITLOG" ] || fail "step3 auth: upstream hit on 401: $(cat "$HITLOG")"
echo "PASS step3: auth 401 + no upstream hit"

# 4. scoped bypass
out=$("${CURL[@]}" -sS "http://127.0.0.1:$PORT/mock-openai/v1/chat/completions" \
  -H 'Authorization: Bearer sk-smoke-123' -H 'content-type: application/json' \
  -d '{"model":"gpt-4o-mini","messages":[{"role":"user","content":"ping"}]}')
echo "$out" | grep -qE '"content":[[:space:]]*"pong"' || fail "step4 scoped: $out"
echo "PASS step4: scoped bypass"

echo "SMOKE PASS"
