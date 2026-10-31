-- The data of the rock-paper-scissors bench. The player of the bench has
-- the id 00000000-0000-4000-8000-000000000001 and one open game, so page /
-- reads the player, the leaderboard and the open game, and writes nothing.

PRAGMA journal_mode = WAL;

CREATE TABLE players (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  wins INTEGER DEFAULT 0,
  draws INTEGER DEFAULT 0,
  games INTEGER DEFAULT 0
);

CREATE TABLE games (
  id TEXT PRIMARY KEY,
  state TEXT DEFAULT 'prepare',
  hash TEXT NOT NULL,
  player TEXT NOT NULL,
  nonce TEXT NOT NULL,
  computer_guess TEXT NOT NULL,
  user_guess TEXT,
  result TEXT,
  FOREIGN KEY (player) REFERENCES players(id)
);

CREATE INDEX games_player_state ON games (player, state);

WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 1000)
INSERT INTO players (id, name, wins, draws, games)
SELECT printf('00000000-0000-4000-8000-%012d', i), 'player ' || i,
       (i * 7) % 50, (i * 3) % 20, 50 + (i * 11) % 100
FROM n;

WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 5000)
INSERT INTO games (id, state, hash, player, nonce, computer_guess, user_guess, result)
SELECT printf('10000000-0000-4000-8000-%012d', i), 'settle',
       lower(hex(randomblob(20))), printf('00000000-0000-4000-8000-%012d', 1 + i % 1000),
       lower(hex(randomblob(4))), 'rock', 'paper', 'win'
FROM n;

INSERT INTO games (id, state, hash, player, nonce, computer_guess)
VALUES ('20000000-0000-4000-8000-000000000001', 'prepare',
        'a94a8fe5ccb19ba61c4c0873d391e987982fbbd3',
        '00000000-0000-4000-8000-000000000001', 'deadbeef', 'rock');
