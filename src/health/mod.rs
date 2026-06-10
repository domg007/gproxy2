//! Per-instance passive health (§3.2/§16.3): member breakers, credential
//! health (breaker + rate-limit cooldown), member latency EWMA. Soft state —
//! restart clears, multi-instance deployments observe independently.

pub mod breaker;
pub mod config;

use std::sync::atomic::{AtomicUsize, Ordering};

use dashmap::DashMap;

use breaker::{Admit, Breaker, Transition};
use config::BreakerConfig;

const EWMA_ALPHA: f64 = 0.3;

struct CredHealth {
    breaker: Breaker,
    cooldown_until: i64,
}

/// Admission verdict for a credential (cooldown folded in).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredAdmit {
    Yes,
    Probe,
    No,
}

/// Entries are created lazily on first touch.
#[derive(Default)]
pub struct HealthState {
    members: DashMap<i64, Breaker>,
    creds: DashMap<i64, CredHealth>,
    /// member_id → latency EWMA (ms).
    latency_ms: DashMap<i64, f64>,
    /// Route/provider rotation counters for round_robin selection.
    rotation: DashMap<i64, AtomicUsize>,
}

impl HealthState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn admit_member(&self, id: i64, cfg: &BreakerConfig, now: i64) -> Admit {
        self.members.entry(id).or_default().admit(cfg, now)
    }

    /// Rate-limit/auth cooldown takes precedence; otherwise the breaker rules.
    pub fn admit_credential(&self, id: i64, cfg: &BreakerConfig, now: i64) -> CredAdmit {
        let mut e = self.creds.entry(id).or_insert_with(|| CredHealth {
            breaker: Breaker::new(),
            cooldown_until: 0,
        });
        if e.cooldown_until > now {
            return CredAdmit::No;
        }
        match e.breaker.admit(cfg, now) {
            Admit::Yes => CredAdmit::Yes,
            Admit::Probe => CredAdmit::Probe,
            Admit::No { .. } => CredAdmit::No,
        }
    }

    pub fn record_member(
        &self,
        id: i64,
        cfg: &BreakerConfig,
        ok: bool,
        now: i64,
    ) -> Option<Transition> {
        let mut b = self.members.entry(id).or_default();
        if ok {
            b.on_success(now)
        } else {
            b.on_failure(cfg, now)
        }
    }

    pub fn record_credential(
        &self,
        id: i64,
        cfg: &BreakerConfig,
        ok: bool,
        now: i64,
    ) -> Option<Transition> {
        let mut e = self.creds.entry(id).or_insert_with(|| CredHealth {
            breaker: Breaker::new(),
            cooldown_until: 0,
        });
        if ok {
            e.breaker.on_success(now)
        } else {
            e.breaker.on_failure(cfg, now)
        }
    }

    /// 429/auth-dead cooldowns; keeps the later of two overlapping deadlines.
    pub fn cool_credential(&self, id: i64, until: i64) {
        let mut e = self.creds.entry(id).or_insert_with(|| CredHealth {
            breaker: Breaker::new(),
            cooldown_until: 0,
        });
        e.cooldown_until = e.cooldown_until.max(until);
    }

    /// EWMA with alpha 0.3; first sample is taken as-is.
    pub fn record_latency(&self, member_id: i64, ms: f64) {
        self.latency_ms
            .entry(member_id)
            .and_modify(|v| *v = *v * (1.0 - EWMA_ALPHA) + ms * EWMA_ALPHA)
            .or_insert(ms);
    }

    pub fn latency(&self, member_id: i64) -> Option<f64> {
        self.latency_ms.get(&member_id).map(|v| *v)
    }

    /// Monotonic per-key counter for round_robin rotation (T2 consumer).
    pub fn next_rotation(&self, key: i64) -> usize {
        self.rotation
            .entry(key)
            .or_default()
            .fetch_add(1, Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cred_cooldown_blocks_until_expiry_and_ewma_tracks_samples() {
        let h = HealthState::new();
        let cfg = BreakerConfig::default();

        h.cool_credential(1, 200);
        assert_eq!(h.admit_credential(1, &cfg, 150), CredAdmit::No);
        assert_eq!(h.admit_credential(1, &cfg, 200), CredAdmit::Yes);

        h.record_latency(7, 100.0);
        assert_eq!(h.latency(7), Some(100.0));
        h.record_latency(7, 200.0);
        assert!((h.latency(7).unwrap() - 130.0).abs() < 1e-9);
        assert_eq!(h.latency(8), None);
    }
}
