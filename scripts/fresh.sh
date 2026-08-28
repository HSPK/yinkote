#!/usr/bin/env bash
# Run the smoke suite against a library that has never been used.
#
# Usage: scripts/fresh.sh [port]
#
# The suite normally runs against `/tmp/ykfinal`, a library it has been feeding
# for dozens of rounds. That made two checks quietly depend on the accumulation
# rather than on the code — one asserted `skipped > 100`, a number only a
# well-fed library reaches — and it meant the thing a *new* install does was
# never exercised at all. See docs/16 §3.201.
#
# It is the same scenario as restoring a backup onto a new machine, so it is
# worth being one command rather than a thing somebody remembers to do.
set -uo pipefail

PORT="${1:-23270}"
DATA=$(mktemp -d /tmp/yk-fresh-XXXXXX)
BIN=./target/release/yinkote
[[ -x "$BIN" ]] || { echo "no $BIN — run: cargo build --release" >&2; exit 1; }

# No agent endpoint on purpose: a new install has no model configured, and the
# checks that need one must say they were skipped rather than fail.
# Backgrounded plainly rather than with `setsid`: this script owns the server
# for its lifetime, so `$!` should *be* the server and not a session leader
# whose child has to be guessed at.
"$BIN" --data-dir "$DATA" --port "$PORT" \
  --web-dir web/dist --plugin-dir plugins > "$DATA/server.log" 2>&1 < /dev/null &
STARTED=$!

# Only ever the process this script started. Killing whatever holds the port
# would mean a mistyped port takes out somebody else's server — and it did:
# the first version of this killed a python http.server that happened to be
# there, while testing what happens when the port is busy.
cleanup() {
  kill "$STARTED" 2>/dev/null
  wait "$STARTED" 2>/dev/null
  [[ "${KEEP:-}" == "1" ]] || rm -rf "$DATA"
}
trap cleanup EXIT

# Ready means *our* server is answering, not that something is. Asking only
# whether the port replies is the "as long as any X exists" mistake (§3.199):
# anything at all listening there would have passed, and then smoke would run
# against it.
ready() {
  [[ "$(curl -sS "http://127.0.0.1:$PORT/api/v1/ping" 2>/dev/null \
        | jq -r '.dataDir // empty' 2>/dev/null)" == "$DATA" ]]
}
for _ in $(seq 1 40); do
  ready && break
  sleep 0.5
done
if ! ready; then
  echo "no Yinkote on $PORT serving $DATA — is something else holding the port?" >&2
  tail -20 "$DATA/server.log" >&2
  exit 1
fi

echo "▸ a library with nothing in it: $DATA"
bash scripts/smoke.sh "http://127.0.0.1:$PORT"
