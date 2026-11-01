// Reads the JSON of oha on stdin and prints one line:
//   <arguments> req/s p50_ms p99_ms load_1min
// It fails when a response has another status than 200.

import { loadavg } from 'node:os';

const r = await Bun.stdin.json();
const codes = Object.keys(r.statusCodeDistribution);

if (codes.join() !== '200') {
  console.error('status codes', JSON.stringify(r.statusCodeDistribution));
  process.exit(1);
}

const p = r.latencyPercentiles;
const ms = (s: number) => Number((s * 1000).toFixed(3));

console.log(...process.argv.slice(2), Math.round(r.summary.requestsPerSec), ms(p.p50), ms(p.p99), loadavg()[0]);
