//! HTTP API.
//!
//! - `POST /api/run` -> `{ token }`: a signed run token, fetched when a game starts.
//! - `POST /api/score` `{ token, player_id, name, time_ms }` -> `{ run_rank, best, board }`.
//! - `GET /api/leaderboard?player=<id>&limit=<n>` -> `Board`.
//! - `GET /health` -> `ok`.
//!
//! Errors are `{ "error": "..." }` worded for the player; the game shows them as is.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::{ConnectInfo, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::{MySqlPool, Row};

use crate::limits::{self, Limiter, Policy};
use crate::{names, token};

/// Longest run accepted (4 hours); anything beyond is not a real game.
const MAX_TIME_MS: u64 = 4 * 3600 * 1000;
/// Shortest run accepted.
const MIN_TIME_MS: u64 = 1000;
/// A run may claim this much more than the server clock saw (request latency, rounding).
const CLOCK_SLACK_MS: u64 = 3000;
/// Tokens older than this are refused (and their nonces forgotten).
pub const TOKEN_MAX_AGE_MS: u64 = MAX_TIME_MS + 3600 * 1000;
const DEFAULT_LIMIT: u32 = 10;
const MAX_LIMIT: u32 = 50;

pub struct AppState {
    pub db: MySqlPool,
    pub secret: Vec<u8>,
    pub trust_proxy: bool,
    pub limiter: Limiter,
}

type Shared = Arc<AppState>;

pub struct ApiError(StatusCode, String, Option<f64>);

impl ApiError {
    fn bad(msg: &str) -> ApiError {
        ApiError(StatusCode::BAD_REQUEST, msg.to_string(), None)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut res = (self.0, Json(json!({ "error": self.1 }))).into_response();
        if let Some(wait) = self.2 {
            if let Ok(v) = HeaderValue::from_str(&(wait.ceil() as u64).max(1).to_string()) {
                res.headers_mut().insert(header::RETRY_AFTER, v);
            }
        }
        res
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> ApiError {
        tracing::error!("database: {e}");
        ApiError(StatusCode::INTERNAL_SERVER_ERROR, "the leaderboard is having a moment; try again soon".into(), None)
    }
}

fn limit(state: &AppState, key: &str, policy: Policy) -> Result<(), ApiError> {
    state.limiter.take(key, policy).map_err(|wait| {
        tracing::info!("rate limited {key}");
        ApiError(
            StatusCode::TOO_MANY_REQUESTS,
            format!("slow down! try again in {}s", wait.ceil() as u64),
            Some(wait),
        )
    })
}

pub fn router(state: Shared) -> Router {
    Router::new()
        .route("/api/run", post(start_run))
        .route("/api/score", post(submit))
        .route("/api/leaderboard", get(leaderboard))
        .route("/health", get(|| async { "ok" }))
        .with_state(state)
}

#[derive(Serialize)]
pub struct Entry {
    pub rank: u32,
    pub name: String,
    pub time_ms: u32,
    pub you: bool,
}

#[derive(Serialize)]
pub struct Board {
    pub top: Vec<Entry>,
    /// The asking player's own standing (their best run), when they have one.
    pub me: Option<Entry>,
    /// Players on the board.
    pub total: u32,
}

fn valid_player_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit())
}

async fn board(db: &MySqlPool, player: Option<&str>, limit: u32) -> Result<Board, ApiError> {
    let total: i64 = sqlx::query("SELECT COUNT(*) AS n FROM players").fetch_one(db).await?.try_get("n")?;
    let rows = sqlx::query("SELECT player_id, name, best_ms FROM players ORDER BY best_ms DESC, best_at ASC LIMIT ?")
        .bind(limit)
        .fetch_all(db)
        .await?;
    let mut top = Vec::with_capacity(rows.len());
    for (i, row) in rows.iter().enumerate() {
        let id: String = row.try_get("player_id")?;
        top.push(Entry {
            rank: i as u32 + 1,
            name: row.try_get("name")?,
            time_ms: row.try_get("best_ms")?,
            you: player == Some(id.as_str()),
        });
    }
    let mut me = None;
    if let Some(p) = player {
        if let Some(row) = sqlx::query("SELECT name, best_ms FROM players WHERE player_id = ?").bind(p).fetch_optional(db).await? {
            let ahead: i64 = sqlx::query(
                "SELECT COUNT(*) AS n FROM players p, (SELECT best_ms, best_at FROM players WHERE player_id = ?) m \
                 WHERE p.best_ms > m.best_ms OR (p.best_ms = m.best_ms AND p.best_at < m.best_at)",
            )
            .bind(p)
            .fetch_one(db)
            .await?
            .try_get("n")?;
            me = Some(Entry { rank: ahead as u32 + 1, name: row.try_get("name")?, time_ms: row.try_get("best_ms")?, you: true });
        }
    }
    Ok(Board { top, me, total: total as u32 })
}

#[derive(Deserialize)]
struct BoardQuery {
    player: Option<String>,
    limit: Option<u32>,
}

async fn leaderboard(
    State(state): State<Shared>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(q): Query<BoardQuery>,
) -> Result<Response, ApiError> {
    let ip = limits::client_key(&headers, peer, state.trust_proxy);
    limit(&state, &format!("general:{ip}"), limits::GENERAL)?;
    let player = q.player.as_deref().filter(|p| valid_player_id(p));
    let board = board(&state.db, player, q.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT)).await?;
    let mut res = Json(board).into_response();
    res.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(res)
}

async fn start_run(
    State(state): State<Shared>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    let ip = limits::client_key(&headers, peer, state.trust_proxy);
    limit(&state, &format!("general:{ip}"), limits::GENERAL)?;
    limit(&state, &format!("run:{ip}"), limits::RUN)?;
    Ok(Json(json!({ "token": token::issue(&state.secret) })))
}

#[derive(Deserialize)]
struct Submit {
    token: String,
    player_id: String,
    name: String,
    time_ms: u64,
}

#[derive(Serialize)]
struct Submitted {
    /// Where this run lands among everyone else's best runs.
    run_rank: u32,
    /// Whether it beat the player's previous best.
    best: bool,
    board: Board,
}

async fn submit(
    State(state): State<Shared>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<Submit>,
) -> Result<Json<Submitted>, ApiError> {
    let ip = limits::client_key(&headers, peer, state.trust_proxy);
    limit(&state, &format!("general:{ip}"), limits::GENERAL)?;
    limit(&state, "score-global", limits::SCORE_GLOBAL)?;
    limit(&state, &format!("score:{ip}"), limits::SCORE)?;
    if !valid_player_id(&body.player_id) {
        return Err(ApiError::bad("bad player id"));
    }
    limit(&state, &format!("score-player:{}", body.player_id), limits::SCORE_PLAYER)?;
    let name = names::clean(&body.name).map_err(ApiError::bad)?;

    let (started, nonce) = token::verify(&state.secret, &body.token).ok_or_else(|| ApiError::bad("that run can't be submitted"))?;
    let now = token::now_ms();
    let elapsed = now.saturating_sub(started);
    if started > now + CLOCK_SLACK_MS || elapsed > TOKEN_MAX_AGE_MS {
        return Err(ApiError::bad("that run is too old to submit"));
    }
    if body.time_ms < MIN_TIME_MS || body.time_ms > MAX_TIME_MS || body.time_ms > elapsed + CLOCK_SLACK_MS {
        tracing::warn!("implausible time {} ms for a {} ms old token from {ip}", body.time_ms, elapsed);
        return Err(ApiError::bad("that time doesn't add up"));
    }
    let time_ms = body.time_ms as u32;

    let mut tx = state.db.begin().await?;
    let fresh = sqlx::query("INSERT IGNORE INTO used_tokens (nonce) VALUES (?)").bind(&nonce).execute(&mut *tx).await?;
    if fresh.rows_affected() == 0 {
        return Err(ApiError::bad("that run was already submitted"));
    }
    let previous: Option<u32> = sqlx::query("SELECT best_ms FROM players WHERE player_id = ? FOR UPDATE")
        .bind(&body.player_id)
        .fetch_optional(&mut *tx)
        .await?
        .map(|r| r.try_get("best_ms"))
        .transpose()?;
    let ip_hash = hex::encode(&Sha256::new().chain_update(&state.secret).chain_update(ip.as_bytes()).finalize()[..8]);
    sqlx::query("INSERT INTO runs (player_id, name, time_ms, ip_hash) VALUES (?, ?, ?, ?)")
        .bind(&body.player_id)
        .bind(&name)
        .bind(time_ms)
        .bind(&ip_hash)
        .execute(&mut *tx)
        .await?;
    // best_at is assigned before best_ms: MariaDB evaluates these left to right with updated values
    sqlx::query(
        "INSERT INTO players (player_id, name, best_ms, best_at, runs) VALUES (?, ?, ?, NOW(3), 1) \
         ON DUPLICATE KEY UPDATE name = VALUES(name), runs = runs + 1, \
         best_at = IF(VALUES(best_ms) > best_ms, VALUES(best_at), best_at), \
         best_ms = GREATEST(best_ms, VALUES(best_ms))",
    )
    .bind(&body.player_id)
    .bind(&name)
    .bind(time_ms)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    let ahead: i64 = sqlx::query("SELECT COUNT(*) AS n FROM players WHERE best_ms > ? AND player_id <> ?")
        .bind(time_ms)
        .bind(&body.player_id)
        .fetch_one(&state.db)
        .await?
        .try_get("n")?;
    let board = board(&state.db, Some(&body.player_id), DEFAULT_LIMIT).await?;
    tracing::info!("score {name:?} {time_ms} ms from {ip_hash}");
    Ok(Json(Submitted { run_rank: ahead as u32 + 1, best: previous.is_none_or(|p| time_ms > p), board }))
}
