// node.mjs in WORKERS processes behind node:cluster. The primary hands out
// the connections round-robin.
//
//   WORKERS=4 node benches/rps/node-cluster.mjs <build-dir> <database>

import cluster from 'node:cluster';

if (cluster.isPrimary) {
  for (let i = 0; i < Number(process.env.WORKERS ?? 1); i++) {
    cluster.fork();
  }
} else {
  await import('./node.mjs');
}
