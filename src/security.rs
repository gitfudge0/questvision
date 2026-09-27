use crate::config::LanAddress;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    net::IpAddr,
    time::{Duration, Instant},
};
use subtle::ConstantTimeEq;

const TTL: Duration = Duration::from_secs(120);
const MAX_ATTEMPTS: u8 = 5;
const START_COOLDOWN: Duration = Duration::from_secs(10);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PairStart {
    pub request_id: String,
}
#[derive(Clone, Debug, Deserialize)]
pub struct PairConfirm {
    pub request_id: String,
    pub code: String,
}
#[derive(Clone, Debug, Serialize)]
pub struct PairToken {
    pub token: String,
}
struct Pending {
    code: String,
    peer: IpAddr,
    expires: Instant,
    attempts: u8,
}

pub struct Pairing {
    pending: HashMap<String, Pending>,
    last_start: HashMap<IpAddr, Instant>,
}
impl Pairing {
    pub fn new() -> Self {
        Self {
            pending: HashMap::new(),
            last_start: HashMap::new(),
        }
    }
    pub fn start(&mut self, peer: IpAddr) -> Option<(PairStart, String)> {
        if !peer.is_loopback() && !peer.is_private_lan() {
            return None;
        }
        let now = Instant::now();
        self.pending.retain(|_, p| p.expires > now);
        if self
            .last_start
            .get(&peer)
            .is_some_and(|last| now.duration_since(*last) < START_COOLDOWN)
        {
            return None;
        }
        self.last_start.insert(peer, now);
        let mut rng = rand::rng();
        let code = format!("{:06}", rng.random_range(0..1_000_000));
        let id = random_hex(16);
        self.pending.insert(
            id.clone(),
            Pending {
                code: code.clone(),
                peer,
                expires: now + TTL,
                attempts: 0,
            },
        );
        Some((PairStart { request_id: id }, code))
    }
    pub fn confirm(&mut self, peer: IpAddr, id: &str, code: &str) -> bool {
        let Some(p) = self.pending.get_mut(id) else {
            return false;
        };
        if p.peer != peer {
            return false;
        }
        if p.expires <= Instant::now() || p.attempts >= MAX_ATTEMPTS {
            self.pending.remove(id);
            return false;
        }
        p.attempts += 1;
        let valid = constant_time_eq(p.code.as_bytes(), code.as_bytes());
        if valid || p.attempts >= MAX_ATTEMPTS {
            self.pending.remove(id);
        }
        valid
    }
}

pub fn random_hex(bytes: usize) -> String {
    let mut out = String::with_capacity(bytes * 2);
    for _ in 0..bytes {
        out.push_str(&format!("{:02x}", rand::random::<u8>()));
    }
    out
}
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    bool::from(a.ct_eq(b))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn code_single_use_and_peer_bound() {
        let ip: IpAddr = "192.168.1.10".parse().unwrap();
        let other = "192.168.1.11".parse().unwrap();
        let mut p = Pairing::new();
        let (request, code) = p.start(ip).unwrap();
        assert!(!p.confirm(other, &request.request_id, &code));
        assert!(p.confirm(ip, &request.request_id, &code));
        assert!(!p.confirm(ip, &request.request_id, &code));
        assert!(p.start(ip).is_none());
    }
    #[test]
    fn wrong_code_exhausts_attempts() {
        let ip = "127.0.0.1".parse().unwrap();
        let mut p = Pairing::new();
        let (request, _) = p.start(ip).unwrap();
        for _ in 0..5 {
            assert!(!p.confirm(ip, &request.request_id, "wrong"));
        }
        assert!(!p.confirm(ip, &request.request_id, "000000"));
    }
}
