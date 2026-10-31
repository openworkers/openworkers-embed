#!/bin/sh
# Start each server on CPUs 0 and 1, measure it with measure.sh (oha on CPUs 2
# and 3), and stop it. Without taskset (macOS), nothing is pinned. Run from
# the root of the crate after the release build of the bench_server example.
#
#   bench/compare.sh [SECONDS] [RUNS] [CASE...]
#
# A case is SERVER:MODE:PATH, for instance embed:app:/ or node:hello:/.
# Server embedw is Embed served on its worker threads (SERVE=workers).
# Pin the server where taskset exists (Linux). macOS has no taskset.
if command -v taskset > /dev/null 2>&1; then
  pin="taskset -c 0,1"
else
  pin=""
fi

seconds=${1:-10}
runs=${2:-3}
shift 2 2>/dev/null
cases=${*:-"embed:app:/ node:app:/ bun:app:/ embed:app:/api/stats node:app:/api/stats bun:app:/api/stats embed:hello:/ node:hello:/ bun:hello:/ embed:axum:/"}

for case in $cases; do
  server=${case%%:*}
  rest=${case#*:}
  mode=${rest%%:*}
  path=${rest#*:}

  case $server in
    embed) port=3000; MODE=$mode $pin ./target/release/examples/bench_server > /dev/null 2>&1 & ;;
    embedw) port=3000; SERVE=workers MODE=$mode $pin ./target/release/examples/bench_server > /dev/null 2>&1 & ;;
    node) port=3001; MODE=$mode $pin node --no-warnings bench/node-server.mjs > /dev/null 2>&1 & ;;
    bun) port=3002; MODE=$mode $pin bun bench/bun-server.ts > /dev/null 2>&1 & ;;
  esac
  pid=$!

  until curl -s -o /dev/null "http://127.0.0.1:$port$path"; do sleep 0.2; done

  echo "== $server $mode $path"
  bench/measure.sh "http://127.0.0.1:$port$path" "$seconds" "$runs"

  kill "$pid"
  wait "$pid" 2>/dev/null
done
