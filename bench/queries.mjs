// The three reads of the page, the same SQL as the Rust commands of
// examples/bench_server.rs.
export const LEADERS = 'SELECT name, wins, losses, draws FROM players ORDER BY wins DESC, id LIMIT 10';
export const RECENT = `SELECT g.id, p.name, g.player, g.server, g.result
  FROM games g JOIN players p ON p.id = g.player_id ORDER BY g.id DESC LIMIT 10`;
export const TOTALS = 'SELECT games, wins, losses, draws FROM totals WHERE id = 1';
