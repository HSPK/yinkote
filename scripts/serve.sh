#!/usr/bin/env bash
# Start a Yinkote server and make sure it is the one you started.
#
# Usage: scripts/serve.sh <data-dir> <port> [--replace]
#
# This exists because starting a server in the background hides the one failure
# that matters. `setsid … &` returns success whether or not the process lived,
# so when the port is already held the new server exits with "Address already in
# use" into a log nobody reads, the old one keeps answering, and everything
# downstream measures or tests a different library. That has now cost three
# separate investigations: a benchmark that ran against the smoke database for
# weeks, a seeding run that filled somebody else's corpus with shelves, and a
# smoke run that reported failures from a binary built before the fix.
set -uo pipefail

DATA="${1:?usage: serve.sh <data-dir> <port> [--replace]}"
PORT="${2:?usage: serve.sh <data-dir> <port> [--replace]}"
REPLACE="${3:-}"

BIN=./target/release/yinkote
[[ -x "$BIN" ]] || { echo "no $BIN — run: cargo build --release" >&2; exit 1; }

held_by() { ss -lptnH "sport = :$PORT" 2>/dev/null | grep -o 'pid=[0-9]*' | cut -d= -f2 | head -1; }

EXISTING=$(held_by)
if [[ -n "$EXISTING" ]]; then
  if [[ "$REPLACE" == "--replace" ]]; then
    echo "▸ stopping the server already on $PORT (pid $EXISTING)"
    kill "$EXISTING"
    for _ in $(seq 1 20); do
      [[ -z "$(held_by)" ]] && break
      sleep 0.5
    done
  else
    # Naming what is there, since the usual mistake is assuming it is yours.
    echo "▸ port $PORT is already held by pid $EXISTING, serving:" >&2
    curl -sS "http://127.0.0.1:$PORT/api/v1/ping" | head -c 200 >&2
    echo -e "\n▸ pass --replace to take it over." >&2
    exit 1
  fi
fi

LOG="/tmp/yinkote-$PORT.log"
setsid env \
  YK_AGENT_ENDPOINT="${YK_AGENT_ENDPOINT:-http://127.0.0.1:8080/v1}" \
  YK_AGENT_MODEL="${YK_AGENT_MODEL:-gpt-5.6-sol_2026-07-09}" \
  "$BIN" --data-dir "$DATA" --port "$PORT" --web-dir web/dist --plugin-dir plugins \
  > "$LOG" 2>&1 &

# Wait for it to answer, then check *what* answered. A server that failed to
# bind leaves the previous one responding, and the two are indistinguishable
# without asking which database is behind it.
for _ in $(seq 1 40); do
  SERVING=$(curl -sS "http://127.0.0.1:$PORT/api/v1/ping" 2>/dev/null | jq -r '.dataDir // empty')
  [[ -n "$SERVING" ]] && break
  sleep 0.5
done

if [[ -z "${SERVING:-}" ]]; then
  echo "▸ nothing is answering on $PORT. Last lines of $LOG:" >&2
  tail -5 "$LOG" >&2
  exit 1
fi

if [[ "$SERVING" != "$DATA" ]]; then
  echo "▸ $PORT is serving $SERVING, not $DATA — the server did not start." >&2
  tail -5 "$LOG" >&2
  exit 1
fi

echo "▸ serving $DATA on $PORT (log: $LOG)"
