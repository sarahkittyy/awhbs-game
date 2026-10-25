//! Run tokens. `POST /api/run` hands one out when a game starts: `<started_ms>.<nonce>.<mac>`,
//! signed with the server secret. A score must come with one, may not claim more time than has
//! passed on the server's clock since it was issued, and each nonce is accepted once. That rules
//! out posting a made-up time without actually leaving a game running that long.

use hmac::{Hmac, Mac};
use rand::RngCore;
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn mac(secret: &[u8], payload: &str) -> HmacSha256 {
    let mut m = HmacSha256::new_from_slice(secret).expect("hmac takes any key length");
    m.update(payload.as_bytes());
    m
}

pub fn issue(secret: &[u8]) -> String {
    let mut nonce = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut nonce);
    let payload = format!("{}.{}", now_ms(), hex::encode(nonce));
    let sig = mac(secret, &payload).finalize().into_bytes();
    format!("{payload}.{}", hex::encode(&sig[..16]))
}

/// The issue time and nonce of a token this server signed.
pub fn verify(secret: &[u8], token: &str) -> Option<(u64, String)> {
    let (payload, sig) = token.rsplit_once('.')?;
    let (started, nonce) = payload.split_once('.')?;
    if nonce.len() != 32 || !nonce.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let sig = hex::decode(sig).ok()?;
    mac(secret, payload).verify_truncated_left(&sig).ok()?;
    Some((started.parse().ok()?, nonce.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_tamper() {
        let t = issue(b"secret");
        let (started, nonce) = verify(b"secret", &t).unwrap();
        assert!(started > 0 && nonce.len() == 32);
        assert!(verify(b"other", &t).is_none());
        let forged = t.replacen(&started.to_string(), &(started - 3_600_000).to_string(), 1);
        assert!(verify(b"secret", &forged).is_none());
    }
}
