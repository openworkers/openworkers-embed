#!/bin/sh
# bun.ts in WORKERS processes on one port, with reusePort.
#
#   WORKERS=4 benches/rps/bun-n.sh <build-dir> <database>
#
# On macOS, SO_REUSEPORT does not spread the connections: one process gets
# them all.

for i in $(seq "${WORKERS:-1}"); do
  REUSE_PORT=1 bun "$(dirname "$0")/bun.ts" "$@" &
done

wait
