# Several threads on page /: Embed with THREADS=n, Node with node:cluster
# and n processes, Bun with n processes on one port (reusePort).
#
#   python3 benches/rps/threads.py <build-dir> <database> [embed-binary]
#
# ROUNDS (3), SECS (10), NS ("1 2 4") and CONNS ("64 256") change the run.
# Each round takes n in turn, and for each n the three servers in turn.
# Before each server, the script waits until no build tool runs (cargo,
# rustc, cc, ld) for 5 s; if one runs during a measure, the server is
# measured again. One line per measure goes to stdout:
#   server n round connections req/s p50_ms p99_ms cpu_percent
#   cpu_percent_per_process load_1min cpu_percent_of_other_processes
# The CPU of a server is the CPU time of its process tree from ps, over the
# wall time of the measure. The last column counts only the processes that
# live through the whole measure, so it misses short ones.

import json
import os
import subprocess
import sys
import threading
import time
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
BUILD, DB = sys.argv[1], sys.argv[2]
EMBED = sys.argv[3] if len(sys.argv) > 3 else f"{ROOT}/target/release/examples/rps"
ROUNDS = int(os.environ.get("ROUNDS", 3))
SECS = int(os.environ.get("SECS", 10))
NS = [int(x) for x in os.environ.get("NS", "1 2 4").split()]
CONNS = [int(x) for x in os.environ.get("CONNS", "64 256").split()]
PORT = os.environ.get("PORT", "3100")
URL = f"http://127.0.0.1:{PORT}/"
COOKIE = "cookie: player=00000000-0000-4000-8000-000000000001"
BUILD_TOOLS = {"cargo", "rustc", "cc", "clang", "ld", "ld64", "rustdoc"}


def command(server, n):
    env = dict(os.environ, PORT=PORT, WORKERS=str(n), THREADS=str(n))
    argv = {
        "embed": [EMBED, BUILD, DB],
        "node": ["node", f"{HERE}/node-cluster.mjs", BUILD, DB],
        "bun": ["sh", f"{HERE}/bun-n.sh", BUILD, DB],
    }[server]

    return argv, env


def up():
    request = urllib.request.Request(URL, headers={"cookie": COOKIE[8:]})

    try:
        urllib.request.urlopen(request, timeout=1)
        return True
    except Exception:
        return False


def seconds(cputime):
    days = 0

    if "-" in cputime:
        days, cputime = cputime.split("-")
        days = int(days)

    total = 0

    for part in cputime.split(":"):
        total = total * 60 + float(part)

    return days * 86400 + total


def ps():
    out = subprocess.run(
        ["ps", "-A", "-o", "pid=,ppid=,cputime=,comm="], capture_output=True, text=True
    ).stdout

    return [line.split(None, 3) for line in out.splitlines()]


def processes():
    """CPU seconds and command of every process."""
    return {int(pid): (seconds(t), comm) for pid, _, t, comm in ps()}


def tree(root):
    """CPU seconds of root and of all its descendants."""
    rows = ps()
    children = {}

    for pid, ppid, t, _ in rows:
        children.setdefault(int(ppid), []).append((int(pid), seconds(t)))

    own = {int(pid): seconds(t) for pid, _, t, _ in rows}
    out, todo = {root: own.get(root, 0)}, [root]

    while todo:
        for pid, t in children.get(todo.pop(), []):
            out[pid] = t
            todo.append(pid)

    return out


def building():
    out = subprocess.run(["ps", "-A", "-o", "comm="], capture_output=True, text=True).stdout

    return any(
        os.path.basename(c.strip()) in BUILD_TOOLS or "build-script" in c
        for c in out.splitlines()
    )


def quiet(seconds_=5):
    calm = 0

    while calm < seconds_ * 2:
        calm = 0 if building() else calm + 1
        time.sleep(0.5)


class Watch:
    """Sets seen when a build tool runs during a measure."""

    def __enter__(self):
        self.seen, self.stop = False, threading.Event()

        def loop():
            while not self.stop.wait(0.3):
                self.seen = self.seen or building()

        self.thread = threading.Thread(target=loop)
        self.thread.start()

        return self

    def __exit__(self, *_):
        self.stop.set()
        self.thread.join()


def oha(connections, secs):
    out = subprocess.run(
        ["oha", "--no-tui", "--output-format", "json", "-z", f"{secs}s",
         "-c", str(connections), "-H", COOKIE, URL],
        capture_output=True, text=True, check=True,
    ).stdout

    return json.loads(out)


def measure(server, n, rnd):
    """The lines of one server, or None when a build tool ran."""
    assert not up(), f"port {PORT} is taken"

    argv, env = command(server, n)
    proc = subprocess.Popen(argv, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

    while not up():
        time.sleep(0.1)

    time.sleep(0.5)
    oha(64, 3)
    lines, dirty = [], False

    for connections in CONNS:
        before, others_before, t0 = tree(proc.pid), processes(), time.time()

        with Watch() as watch:
            r = oha(connections, SECS)

        after, others_after, t1 = tree(proc.pid), processes(), time.time()
        dirty = dirty or watch.seen
        assert list(r["statusCodeDistribution"]) == ["200"], r["statusCodeDistribution"]

        wall = t1 - t0
        per = sorted((round(100 * (after[k] - before.get(k, 0)) / wall) for k in after), reverse=True)
        per = [x for x in per if x > 0]
        rest = sum(
            t - others_before[k][0]
            for k, (t, comm) in others_after.items()
            if k in others_before and k not in after and not comm.endswith("oha")
        )
        q = r["latencyPercentiles"]
        lines.append(" ".join(map(str, (
            server, n, rnd, connections, round(r["summary"]["requestsPerSec"]),
            round(q["p50"] * 1000, 3), round(q["p99"] * 1000, 3), sum(per),
            ",".join(map(str, per)), round(os.getloadavg()[0], 1), round(100 * rest / wall),
        ))))

    # The children first: a child of a stopped shell would be left running
    for pid in sorted(tree(proc.pid), key=lambda pid: pid == proc.pid):
        try:
            os.kill(pid, 15)
        except ProcessLookupError:
            pass

    proc.wait()

    while up():
        time.sleep(0.1)

    return None if dirty else lines


for rnd in range(1, ROUNDS + 1):
    for n in NS:
        for server in ["embed", "node", "bun"]:
            for _ in range(20):
                quiet()
                lines = measure(server, n, rnd)

                if lines:
                    print("\n".join(lines), flush=True)
                    break

                print("again", server, n, rnd, file=sys.stderr, flush=True)
