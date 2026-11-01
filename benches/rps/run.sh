#!/bin/sh
# The baseline of the rock-paper-scissors bench: Embed, Node and Bun on page /,
# with the same build, the same database and the same oha settings.
#
#   benches/rps/run.sh <build-dir> <database> [rounds] [seconds]
#
# Each round starts each server, warms it, then measures one connection and
# 64 connections. The servers take turns in each round, so a change of the
# machine load falls on all of them. With BASE=<binary> and "base" in
# SERVERS, an older build of examples/rps runs in turn with the new one.
# One line per measure goes to stdout:
#   server round connections req/s p50_ms p99_ms load_1min

set -eu

BUILD=$1
DATABASE=$2
ROUNDS=${3:-5}
SECONDS_PER_RUN=${4:-10}
PORT=${PORT:-3100}
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
COOKIE='cookie: player=00000000-0000-4000-8000-000000000001'
URL="http://127.0.0.1:$PORT/"
SERVERS=${SERVERS:-"embed node bun"}

start() {
  case $1 in
    embed) PORT=$PORT exec "$ROOT/target/release/examples/rps" "$BUILD" "$DATABASE" ;;
    base) PORT=$PORT exec "$BASE" "$BUILD" "$DATABASE" ;;
    node) PORT=$PORT exec node "$ROOT/benches/rps/node.mjs" "$BUILD" "$DATABASE" ;;
    bun) PORT=$PORT exec bun "$ROOT/benches/rps/bun.ts" "$BUILD" "$DATABASE" ;;
  esac
}

measure() {
  oha --no-tui --output-format json -z "${SECONDS_PER_RUN}s" -c "$3" -H "$COOKIE" "$URL" |
    bun "$ROOT/benches/rps/oha.ts" "$1" "$2" "$3"
}

for round in $(seq "$ROUNDS"); do
  for server in $SERVERS; do
    if curl -s -o /dev/null "$URL"; then
      echo "port $PORT is taken" >&2
      exit 1
    fi

    start "$server" >/dev/null 2>&1 &
    pid=$!

    until curl -s -o /dev/null "$URL"; do sleep 0.1; done

    oha --no-tui --output-format quiet -z 3s -c 64 -H "$COOKIE" "$URL"
    measure "$server" "$round" 1
    measure "$server" "$round" 64

    kill "$pid"
    wait "$pid" 2>/dev/null || true
  done
done
