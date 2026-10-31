#!/bin/sh
# The layer ladder: each case adds one layer to the one before. One line per
# measure goes to stdout:
#   case round connections req/s p50_ms p99_ms load_1min
#
#   benches/rps/ladder.sh <assets-dir> [rounds] [seconds]

set -eu

ASSETS_DIR=$1
ROUNDS=${2:-5}
SECONDS_PER_RUN=${3:-5}
PORT=${PORT:-3100}
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
LADDER="$ROOT/target/release/examples/ladder"
WORKERS="$ROOT/benches/rps/ladder"
URL="http://127.0.0.1:$PORT/"
CASES=${CASES:-"e-axum e-hop e-hello e-hello-assets e-stream e-invoke3 n-raw n-hello n-stream n-invoke3"}

start() {
  export PORT

  case $1 in
    e-axum) exec "$LADDER" axum ;;
    e-hop) exec "$LADDER" hop ;;
    e-hello) exec "$LADDER" embed "$WORKERS/hello.js" ;;
    e-hello-assets) ASSETS=$ASSETS_DIR exec "$LADDER" embed "$WORKERS/hello.js" ;;
    e-stream) exec "$LADDER" embed "$WORKERS/stream.js" ;;
    e-invoke3) exec "$LADDER" embed "$WORKERS/invoke3.js" ;;
    n-raw) exec node "$ROOT/benches/rps/ladder.mjs" raw ;;
    n-hello) exec node "$ROOT/benches/rps/ladder.mjs" "$WORKERS/hello.js" ;;
    n-stream) exec node "$ROOT/benches/rps/ladder.mjs" "$WORKERS/stream.js" ;;
    n-invoke3) exec node "$ROOT/benches/rps/ladder.mjs" "$WORKERS/invoke3.js" ;;
    b-raw) exec bun "$ROOT/benches/rps/ladder-bun.ts" raw ;;
    b-hello) exec bun "$ROOT/benches/rps/ladder-bun.ts" "$WORKERS/hello.js" ;;
    b-invoke3) exec bun "$ROOT/benches/rps/ladder-bun.ts" "$WORKERS/invoke3.js" ;;
  esac
}

measure() {
  oha --no-tui --output-format json -z "${SECONDS_PER_RUN}s" -c "$3" "$URL" |
    python3 -c '
import json, os, sys
r = json.load(sys.stdin)
codes = r["statusCodeDistribution"]
assert list(codes) == ["200"], codes
p = r["latencyPercentiles"]
print(sys.argv[1], sys.argv[2], sys.argv[3], round(r["summary"]["requestsPerSec"]),
      round(p["p50"] * 1000, 3), round(p["p99"] * 1000, 3), os.getloadavg()[0], flush=True)
' "$1" "$2" "$3"
}

for round in $(seq "$ROUNDS"); do
  for case in $CASES; do
    if curl -s -o /dev/null "$URL"; then
      echo "port $PORT is taken" >&2
      exit 1
    fi

    (start "$case") >/dev/null 2>&1 &
    pid=$!

    until curl -s -o /dev/null "$URL"; do sleep 0.1; done

    oha --no-tui --output-format quiet -z 2s -c 64 "$URL"
    measure "$case" "$round" 1
    measure "$case" "$round" 64

    kill "$pid"
    wait "$pid" 2>/dev/null || true
  done
done
