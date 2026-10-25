//! Rate limiting: leaky buckets keyed by client address (and a few by player or for the whole
//! server). A bucket holds `level` units that drain at `per_sec`; each request pours one unit in
//! and is refused when that would overflow `capacity`. So an idle bucket lets a burst of
//! `capacity` through, after which the sustained rate is `per_sec`.
//!
//! | bucket | routes | per | burst | sustained |
//! | --- | --- | --- | --- | --- |
//! | general | everything under `/api` | address | 40 | 4 / s |
//! | run | `POST /api/run` (one per game started) | address | 10 | 15 / min |
//! | score | `POST /api/score` | address | 4 | 3 / min |
//! | score-player | `POST /api/score` | player id | 3 | 3 / min |
//! | score-global | `POST /api/score` | whole server | 60 | 2 / s |

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Mutex;
use std::time::Instant;

use axum::http::HeaderMap;

#[derive(Clone, Copy, Debug)]
pub struct Policy {
    pub capacity: f64,
    pub per_sec: f64,
}

impl Policy {
    /// A burst of `capacity`, then `n` requests every `secs` seconds.
    pub const fn per(capacity: f64, n: f64, secs: f64) -> Policy {
        Policy { capacity, per_sec: n / secs }
    }
}

pub const GENERAL: Policy = Policy::per(40.0, 4.0, 1.0);
pub const RUN: Policy = Policy::per(10.0, 15.0, 60.0);
pub const SCORE: Policy = Policy::per(4.0, 3.0, 60.0);
pub const SCORE_PLAYER: Policy = Policy::per(3.0, 3.0, 60.0);
pub const SCORE_GLOBAL: Policy = Policy::per(60.0, 2.0, 1.0);

/// Idle buckets are swept when this many exist; ~100 bytes each.
const MAX_BUCKETS: usize = 50_000;

struct Bucket {
    level: f64,
    per_sec: f64,
    at: Instant,
}

impl Bucket {
    fn level_at(&self, now: Instant) -> f64 {
        (self.level - now.duration_since(self.at).as_secs_f64() * self.per_sec).max(0.0)
    }
}

pub struct Limiter {
    pub enabled: bool,
    buckets: Mutex<HashMap<String, Bucket>>,
}

impl Limiter {
    pub fn new(enabled: bool) -> Limiter {
        Limiter { enabled, buckets: Mutex::new(HashMap::new()) }
    }

    /// Pours one request into `key`'s bucket. `Err` carries the seconds until it would fit.
    pub fn take(&self, key: &str, policy: Policy) -> Result<(), f64> {
        if !self.enabled {
            return Ok(());
        }
        let now = Instant::now();
        let mut map = self.buckets.lock().unwrap();
        if map.len() >= MAX_BUCKETS && !map.contains_key(key) {
            map.retain(|_, b| b.level_at(now) > 0.0);
        }
        let b = map
            .entry(key.to_string())
            .or_insert(Bucket { level: 0.0, per_sec: policy.per_sec, at: now });
        b.level = b.level_at(now);
        b.per_sec = policy.per_sec;
        b.at = now;
        if b.level + 1.0 > policy.capacity + 1e-9 {
            return Err((b.level + 1.0 - policy.capacity) / policy.per_sec);
        }
        b.level += 1.0;
        Ok(())
    }

    /// Drops buckets that have fully drained.
    pub fn sweep(&self) {
        let now = Instant::now();
        self.buckets.lock().unwrap().retain(|_, b| b.level_at(now) > 0.0);
    }
}

/// The client's address: the peer, or nginx's `X-Real-IP` when behind the proxy. IPv6 is cut to
/// its /64 so one SLAAC prefix shares a bucket.
pub fn client_key(headers: &HeaderMap, peer: SocketAddr, trust_proxy: bool) -> String {
    let ip = trust_proxy
        .then(|| headers.get("x-real-ip")?.to_str().ok()?.trim().parse::<IpAddr>().ok())
        .flatten()
        .unwrap_or(peer.ip());
    match ip {
        IpAddr::V4(v4) => v4.to_string(),
        IpAddr::V6(v6) => {
            let s = v6.segments();
            format!("{:x}:{:x}:{:x}:{:x}::/64", s[0], s[1], s[2], s[3])
        }
    }
}
