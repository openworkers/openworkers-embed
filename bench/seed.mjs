// Make bench/rps.db: 1000 players, 20000 games, and the totals in one row.
// Run it with `node seed.mjs`.
import { DatabaseSync } from 'node:sqlite';
import { rmSync } from 'node:fs';

const path = new URL('rps.db', import.meta.url).pathname;

rmSync(path, { force: true });

const db = new DatabaseSync(path);
const moves = ['rock', 'paper', 'scissors'];
const results = ['won', 'lost', 'drawn'];

db.exec(`
  CREATE TABLE players (id INTEGER PRIMARY KEY, name TEXT NOT NULL, wins INTEGER NOT NULL,
    losses INTEGER NOT NULL, draws INTEGER NOT NULL);
  CREATE INDEX players_wins ON players (wins);
  CREATE TABLE games (id INTEGER PRIMARY KEY, player_id INTEGER NOT NULL,
    player TEXT NOT NULL, server TEXT NOT NULL, result TEXT NOT NULL);
  CREATE TABLE totals (id INTEGER PRIMARY KEY, games INTEGER, wins INTEGER, losses INTEGER,
    draws INTEGER);
`);

// A fixed seed, so that every run reads the same rows
let state = 42;
const random = (n) => {
  state = (state * 1103515245 + 12345) % 2147483648;
  return state % n;
};

const players = Array.from({ length: 1000 }, (_, i) => ({ id: i + 1, wins: 0, losses: 0, draws: 0 }));
const totals = { won: 0, lost: 0, drawn: 0 };
const game = db.prepare('INSERT INTO games (player_id, player, server, result) VALUES (?, ?, ?, ?)');

db.exec('BEGIN');
for (let i = 0; i < 20000; i++) {
  const player = players[random(players.length)];
  const result = results[random(3)];

  game.run(player.id, moves[random(3)], moves[random(3)], result);
  totals[result]++;
  if (result === 'won') player.wins++;
  else if (result === 'lost') player.losses++;
  else player.draws++;
}

const insert = db.prepare('INSERT INTO players VALUES (?, ?, ?, ?, ?)');
for (const p of players) insert.run(p.id, `player-${p.id}`, p.wins, p.losses, p.draws);

db.prepare('INSERT INTO totals VALUES (1, ?, ?, ?, ?)').run(20000, totals.won, totals.lost, totals.drawn);
db.exec('COMMIT');
db.close();
