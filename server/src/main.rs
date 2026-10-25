//! awhbs leaderboard server. Configured from the environment (`.env` in the working directory
//! is read first; see `.env.example`).

mod api;
mod limits;
mod names;
mod token;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use sqlx::mysql::MySqlPoolOptions;
use tracing_subscriber::EnvFilter;

fn var(name: &str) -> Option<String> {
    std::env::var(name).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

fn flag(name: &str, default: bool) -> bool {
    match var(name).map(|v| v.to_ascii_lowercase()).as_deref() {
        Some("1" | "true" | "on" | "yes") => true,
        Some("0" | "false" | "off" | "no") => false,
        _ => default,
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = dotenvy::dotenv();
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,sqlx=warn")))
        .init();

    let host: IpAddr = var("AWHBS_HOST").map(|h| h.parse()).transpose()?.unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST));
    let port: u16 = var("AWHBS_PORT").map(|p| p.parse()).transpose()?.unwrap_or(3547);
    let database_url = var("DATABASE_URL").ok_or("DATABASE_URL is not set (see .env.example)")?;
    let secret = var("AWHBS_SECRET").ok_or("AWHBS_SECRET is not set (any long random string)")?;

    let db = MySqlPoolOptions::new()
        .max_connections(8)
        .acquire_timeout(Duration::from_secs(5))
        .connect(&database_url)
        .await?;
    sqlx::migrate!("./migrations").run(&db).await?;

    let state = Arc::new(api::AppState {
        db: db.clone(),
        secret: secret.into_bytes(),
        trust_proxy: flag("AWHBS_TRUST_PROXY", false),
        limiter: limits::Limiter::new(flag("AWHBS_RATE_LIMITS", true)),
    });

    // hourly: forget nonces of tokens too old to submit anyway, and drained rate buckets
    {
        let state = state.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(3600));
            loop {
                tick.tick().await;
                state.limiter.sweep();
                let age_s = api::TOKEN_MAX_AGE_MS / 1000;
                if let Err(e) = sqlx::query("DELETE FROM used_tokens WHERE used_at < NOW(3) - INTERVAL ? SECOND")
                    .bind(age_s)
                    .execute(&state.db)
                    .await
                {
                    tracing::warn!("token cleanup: {e}");
                }
            }
        });
    }

    let mut app = api::router(state);
    // local development: serve the web build from the same origin (production uses nginx)
    if let Some(dir) = var("AWHBS_STATIC") {
        tracing::info!("serving static files from {dir}");
        app = app.fallback_service(tower_http::services::ServeDir::new(dir));
    }

    let addr = SocketAddr::new(host, port);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("awhbs-server listening on {addr}");
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await?;
    Ok(())
}
