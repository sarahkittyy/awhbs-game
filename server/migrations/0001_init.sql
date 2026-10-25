-- One row per player (a random id the client keeps in local storage): their best run and the
-- name they last submitted under. The leaderboard reads only this table.
CREATE TABLE IF NOT EXISTS players (
    player_id CHAR(32) NOT NULL PRIMARY KEY,
    name VARCHAR(16) NOT NULL,
    best_ms INT UNSIGNED NOT NULL,
    best_at DATETIME(3) NOT NULL,
    runs INT UNSIGNED NOT NULL DEFAULT 0,
    KEY by_best (best_ms DESC, best_at)
);

-- Every accepted submission, for moderation (ip_hash is a salted hash, never the address).
CREATE TABLE IF NOT EXISTS runs (
    id BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
    player_id CHAR(32) NOT NULL,
    name VARCHAR(16) NOT NULL,
    time_ms INT UNSIGNED NOT NULL,
    ip_hash CHAR(16) NOT NULL,
    created_at DATETIME(3) NOT NULL DEFAULT CURRENT_TIMESTAMP(3),
    KEY by_player (player_id),
    KEY by_ip (ip_hash, created_at)
);

-- Run tokens are single use: a submission records the token's nonce here.
CREATE TABLE IF NOT EXISTS used_tokens (
    nonce CHAR(32) NOT NULL PRIMARY KEY,
    used_at DATETIME(3) NOT NULL DEFAULT CURRENT_TIMESTAMP(3)
);
