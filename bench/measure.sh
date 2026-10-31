#!/bin/sh
# Measure one server that already listens: the median at one connection and
# the requests per second at 64 connections, with oha.
#
#   bench/measure.sh URL [SECONDS] [RUNS]
#
# Each run prints one line: p50 at 1 connection (ms), p99 at 1 connection
# (ms), requests per second at 64 connections.
set -e
url=$1
seconds=${2:-10}

# oha runs on CPUs 2 and 3 where taskset exists (Linux). macOS has no taskset.
if command -v taskset > /dev/null 2>&1; then
  pin="taskset -c 2,3"
else
  pin=""
fi
runs=${3:-3}

# Warm the server: JIT, caches and contexts
$pin oha --no-tui -z 3s -c 64 "$url" > /dev/null

for run in $(seq "$runs"); do
  one=$($pin oha --no-tui --output-format json -z "${seconds}s" -c 1 "$url")
  many=$($pin oha --no-tui --output-format json -z "${seconds}s" -c 64 "$url")
  echo "$one" "$many" | python3 -c '
import json, sys
decoder = json.JSONDecoder()
text = sys.stdin.read().strip()
one, end = decoder.raw_decode(text)
many, _ = decoder.raw_decode(text[end:].strip())
ok = lambda r: sum(v for k, v in r["statusCodeDistribution"].items() if k == "200")
print("p50 %.3f ms  p99 %.3f ms  (1 conn, %d ok)   %.0f req/s  (64 conn, %d ok)" % (
  one["latencyPercentiles"]["p50"] * 1000, one["latencyPercentiles"]["p99"] * 1000, ok(one),
  many["summary"]["requestsPerSec"], ok(many)))
'
done
