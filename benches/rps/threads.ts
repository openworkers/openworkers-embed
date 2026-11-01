// Several threads on page /: Embed with THREADS=n, Node with node:cluster
// and n processes, Bun with n processes on one port (reusePort).
//
//   bun benches/rps/threads.ts <build-dir> <database> [embed-binary]
//
// ROUNDS (3), SECS (10), NS ("1 2 4") and CONNS ("64 256") change the run.
// Each round takes n in turn, and for each n the three servers in turn.
// Before each server, the script waits until no build tool runs (cargo,
// rustc, cc, ld) for 5 s; if one runs during a measure, the server is
// measured again. One line per measure goes to stdout:
//   server n round connections req/s p50_ms p99_ms cpu_percent
//   cpu_percent_per_process load_1min cpu_percent_of_other_processes
// The CPU of a server is the CPU time of its process tree from ps, over the
// wall time of the measure. The last column counts only the processes that
// live through the whole measure, so it misses short ones.

import { loadavg } from 'node:os';
import { basename, resolve } from 'node:path';

const HERE = import.meta.dir;
const ROOT = resolve(HERE, '../..');
const [BUILD, DB, EMBED = `${ROOT}/target/release/examples/rps`] = process.argv.slice(2);

if (!BUILD || !DB) {
  console.error('usage: bun benches/rps/threads.ts <build-dir> <database> [embed-binary]');
  process.exit(1);
}

const list = (value: string) => value.split(/\s+/).filter(Boolean).map(Number);
const ROUNDS = Number(process.env.ROUNDS ?? 3);
const SECS = Number(process.env.SECS ?? 10);
const NS = list(process.env.NS ?? '1 2 4');
const CONNS = list(process.env.CONNS ?? '64 256');
const PORT = process.env.PORT ?? '3100';
const TARGET = `http://127.0.0.1:${PORT}/`;
const COOKIE = 'player=00000000-0000-4000-8000-000000000001';
const BUILD_TOOLS = new Set(['cargo', 'rustc', 'cc', 'clang', 'ld', 'ld64', 'rustdoc']);
const SERVERS = ['embed', 'node', 'bun'] as const;

type Server = (typeof SERVERS)[number];
type Process = { pid: number; ppid: number; cpu: number; comm: string };

const fixed = (value: number, digits: number) => Number(value.toFixed(digits));

function command(server: Server, n: number) {
  const env = { ...process.env, PORT, WORKERS: String(n), THREADS: String(n) };
  const argv = {
    embed: [EMBED, BUILD, DB],
    node: ['node', `${HERE}/node-cluster.mjs`, BUILD, DB],
    bun: ['sh', `${HERE}/bun-n.sh`, BUILD, DB]
  }[server];

  return { argv, env };
}

async function up() {
  try {
    const response = await fetch(TARGET, {
      headers: { cookie: COOKIE, connection: 'close' },
      signal: AbortSignal.timeout(1000)
    });

    await response.arrayBuffer();

    return true;
  } catch {
    return false;
  }
}

/** Seconds of a ps cputime: [days-][hours:]minutes:seconds. */
function seconds(cputime: string) {
  let days = 0;

  if (cputime.includes('-')) {
    const [d, rest] = cputime.split('-');
    days = Number(d);
    cputime = rest;
  }

  let total = 0;

  for (const part of cputime.split(':')) {
    total = total * 60 + Number(part);
  }

  return days * 86400 + total;
}

function ps(): Process[] {
  const out = Bun.spawnSync(['ps', '-A', '-o', 'pid=,ppid=,cputime=,comm=']).stdout.toString();

  return out
    .split('\n')
    .filter((line) => line.trim())
    .map((line) => {
      const [pid, ppid, cputime, ...comm] = line.trim().split(/\s+/);

      return { pid: Number(pid), ppid: Number(ppid), cpu: seconds(cputime), comm: comm.join(' ') };
    });
}

/** CPU seconds and command of every process. */
function processes() {
  return new Map(ps().map((p) => [p.pid, p]));
}

/** CPU seconds of root and of all its descendants. */
function tree(root: number) {
  const rows = ps();
  const children = new Map<number, Process[]>();

  for (const p of rows) {
    children.set(p.ppid, [...(children.get(p.ppid) ?? []), p]);
  }

  const out = new Map([[root, rows.find((p) => p.pid === root)?.cpu ?? 0]]);
  const todo = [root];

  while (todo.length) {
    for (const p of children.get(todo.pop()!) ?? []) {
      out.set(p.pid, p.cpu);
      todo.push(p.pid);
    }
  }

  return out;
}

function building() {
  const out = Bun.spawnSync(['ps', '-A', '-o', 'comm=']).stdout.toString();

  return out
    .split('\n')
    .some((c) => BUILD_TOOLS.has(basename(c.trim())) || c.includes('build-script'));
}

async function quiet(secs = 5) {
  let calm = 0;

  while (calm < secs * 2) {
    calm = building() ? 0 : calm + 1;
    await Bun.sleep(500);
  }
}

/** The result of work, and whether a build tool ran while it ran. */
async function watched<T>(work: Promise<T>) {
  let seen = false;
  const timer = setInterval(() => {
    seen ||= building();
  }, 300);

  try {
    const result = await work;

    return { result, seen };
  } finally {
    clearInterval(timer);
  }
}

async function oha(connections: number, secs: number) {
  const proc = Bun.spawn(
    ['oha', '--no-tui', '--output-format', 'json', '-z', `${secs}s`, '-c', String(connections), '-H', `cookie: ${COOKIE}`, TARGET],
    { stdout: 'pipe' }
  );
  const out = await new Response(proc.stdout).text();

  if ((await proc.exited) !== 0) {
    throw new Error(`oha exited with ${proc.exitCode}`);
  }

  return JSON.parse(out);
}

/** The lines of one server, or null when a build tool ran. */
async function measure(server: Server, n: number, round: number) {
  if (await up()) {
    throw new Error(`port ${PORT} is taken`);
  }

  const { argv, env } = command(server, n);
  const proc = Bun.spawn(argv, { env, stdout: 'ignore', stderr: 'ignore' });

  while (!(await up())) {
    await Bun.sleep(100);
  }

  await Bun.sleep(500);
  await oha(64, 3);
  const lines: string[] = [];
  let dirty = false;

  for (const connections of CONNS) {
    const before = tree(proc.pid);
    const othersBefore = processes();
    const t0 = performance.now();
    const { result: r, seen } = await watched(oha(connections, SECS));
    const after = tree(proc.pid);
    const othersAfter = processes();
    const wall = (performance.now() - t0) / 1000;
    dirty ||= seen;

    const codes = Object.keys(r.statusCodeDistribution);

    if (codes.join() !== '200') {
      throw new Error(`status codes ${JSON.stringify(r.statusCodeDistribution)}`);
    }

    const per = [...after]
      .map(([pid, cpu]) => Math.round((100 * (cpu - (before.get(pid) ?? 0))) / wall))
      .filter((x) => x > 0)
      .sort((a, b) => b - a);
    let rest = 0;

    for (const [pid, p] of othersAfter) {
      const old = othersBefore.get(pid);

      if (old && !after.has(pid) && !p.comm.endsWith('oha')) {
        rest += p.cpu - old.cpu;
      }
    }

    const q = r.latencyPercentiles;
    lines.push(
      [
        server,
        n,
        round,
        connections,
        Math.round(r.summary.requestsPerSec),
        fixed(q.p50 * 1000, 3),
        fixed(q.p99 * 1000, 3),
        per.reduce((a, b) => a + b, 0),
        per.join(','),
        fixed(loadavg()[0], 1),
        Math.round((100 * rest) / wall)
      ].join(' ')
    );
  }

  // The children first: a child of a stopped shell would be left running
  const pids = [...tree(proc.pid).keys()].sort((a, b) => Number(a === proc.pid) - Number(b === proc.pid));

  for (const pid of pids) {
    try {
      process.kill(pid, 'SIGTERM');
    } catch {}
  }

  await proc.exited;

  while (await up()) {
    await Bun.sleep(100);
  }

  return dirty ? null : lines;
}

for (let round = 1; round <= ROUNDS; round++) {
  for (const n of NS) {
    for (const server of SERVERS) {
      for (let attempt = 0; attempt < 20; attempt++) {
        await quiet();
        const lines = await measure(server, n, round);

        if (lines) {
          console.log(lines.join('\n'));
          break;
        }

        console.error('again', server, n, round);
      }
    }
  }
}
