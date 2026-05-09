use crate::models::ArbCandidate;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

const DEFAULT_TTL_MS: u64 = 6 * 60 * 60 * 1000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FamilyDedupEntry {
    pub key: String,
    pub sport: String,
    pub event_id: String,
    pub soft_venue: String,
    pub market_type: String,
    pub period: String,
    pub reason: String,
    pub locked_at_ms: u64,
    pub expires_at_ms: u64,
}

#[derive(Debug, Clone)]
pub struct FamilyDedupHit {
    pub key: String,
    pub reason: String,
    pub remaining_secs: u64,
}

pub struct FamilyDedup {
    entries: HashMap<String, FamilyDedupEntry>,
    path: PathBuf,
    ttl_ms: u64,
}

impl FamilyDedup {
    pub fn ttl_secs_from_env() -> u64 {
        ttl_ms_from_env() / 1000
    }

    pub fn load_from_env() -> Self {
        let path = std::env::var("FAMILY_DEDUP_FILE")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("data/family-dedup.json"));
        let ttl_ms = ttl_ms_from_env();
        let mut dedup = Self {
            entries: HashMap::new(),
            path,
            ttl_ms,
        };
        dedup.load();
        dedup.prune_expired();
        dedup
    }

    pub fn ttl_secs(&self) -> u64 {
        self.ttl_ms / 1000
    }

    pub fn lock_status(&mut self, candidate: &ArbCandidate) -> Option<FamilyDedupHit> {
        self.prune_expired();
        let key = family_key(candidate);
        let now = now_ms();
        self.entries.get(&key).and_then(|entry| {
            if entry.expires_at_ms <= now {
                None
            } else {
                Some(FamilyDedupHit {
                    key,
                    reason: entry.reason.clone(),
                    remaining_secs: ((entry.expires_at_ms - now) + 999) / 1000,
                })
            }
        })
    }

    pub fn record_lock(
        &mut self,
        candidate: &ArbCandidate,
        reason: impl Into<String>,
    ) -> FamilyDedupEntry {
        let now = now_ms();
        let key = family_key(candidate);
        let entry = FamilyDedupEntry {
            key: key.clone(),
            sport: candidate.sport.clone(),
            event_id: candidate.event_id.clone(),
            soft_venue: soft_venue(candidate),
            market_type: candidate.market_type.clone(),
            period: period_label(candidate),
            reason: reason.into(),
            locked_at_ms: now,
            expires_at_ms: now + self.ttl_ms,
        };
        self.entries.insert(key, entry.clone());
        self.save();
        entry
    }

    pub fn prune_expired(&mut self) {
        let now = now_ms();
        let before = self.entries.len();
        self.entries.retain(|_, entry| entry.expires_at_ms > now);
        if self.entries.len() != before {
            self.save();
        }
    }

    fn load(&mut self) {
        let Ok(raw) = fs::read_to_string(&self.path) else {
            return;
        };
        let Ok(entries) = serde_json::from_str::<Vec<FamilyDedupEntry>>(&raw) else {
            tracing::warn!(
                "[FAMILY DEDUP] Could not parse {}; starting with empty state",
                self.path.display()
            );
            return;
        };
        self.entries = entries
            .into_iter()
            .map(|entry| (entry.key.clone(), entry))
            .collect();
    }

    fn save(&self) {
        if let Some(parent) = self.path.parent() {
            if let Err(e) = fs::create_dir_all(parent) {
                tracing::warn!(
                    "[FAMILY DEDUP] Could not create {}: {}",
                    parent.display(),
                    e
                );
                return;
            }
        }

        let mut entries: Vec<_> = self.entries.values().cloned().collect();
        entries.sort_by(|a, b| a.key.cmp(&b.key));
        match serde_json::to_string_pretty(&entries) {
            Ok(raw) => {
                if let Err(e) = fs::write(&self.path, raw) {
                    tracing::warn!(
                        "[FAMILY DEDUP] Could not write {}: {}",
                        self.path.display(),
                        e
                    );
                }
            }
            Err(e) => tracing::warn!("[FAMILY DEDUP] Could not serialize state: {}", e),
        }
    }
}

pub fn family_key(candidate: &ArbCandidate) -> String {
    format!(
        "{}|{}|{}|{}|{}",
        candidate.sport.to_ascii_lowercase(),
        candidate.event_id.to_ascii_lowercase(),
        soft_venue(candidate),
        candidate.market_type.to_ascii_lowercase(),
        period_label(candidate).to_ascii_lowercase(),
    )
}

fn soft_venue(candidate: &ArbCandidate) -> String {
    if candidate.leg1_venue != "pinnacle" {
        candidate.leg1_venue.to_ascii_lowercase()
    } else if candidate.leg2_venue != "pinnacle" {
        candidate.leg2_venue.to_ascii_lowercase()
    } else {
        let mut venues = [candidate.leg1_venue.as_str(), candidate.leg2_venue.as_str()];
        venues.sort_unstable();
        venues.join("+").to_ascii_lowercase()
    }
}

fn period_label(candidate: &ArbCandidate) -> String {
    let period = candidate.period.trim();
    if period.is_empty() {
        "unknown".to_string()
    } else {
        period.to_string()
    }
}

fn ttl_ms_from_env() -> u64 {
    for name in ["FAMILY_DEDUP_TTL_MS", "DEDUP_COOLDOWN_MS"] {
        if let Some(v) = env_u64(name) {
            return v.max(1);
        }
    }
    for name in ["FAMILY_DEDUP_TTL_SECS", "DEDUP_COOLDOWN_SECS"] {
        if let Some(v) = env_u64(name) {
            return v.saturating_mul(1000).max(1);
        }
    }
    DEFAULT_TTL_MS
}

fn env_u64(name: &str) -> Option<u64> {
    std::env::var(name).ok().and_then(|v| v.parse::<u64>().ok())
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::family_key;
    use crate::models::ArbCandidate;

    fn candidate(leg1_venue: &str, leg2_venue: &str, period: &str) -> ArbCandidate {
        ArbCandidate {
            event_id: "UFC:2026-05-09T17:JOABUC-vs-SEABRA".to_string(),
            sport: "UFC".to_string(),
            market_type: "moneyline".to_string(),
            period: period.to_string(),
            start_time_utc: "2026-05-09T17:00:00Z".to_string(),
            leg1_venue: leg1_venue.to_string(),
            leg1_selection: "JOABUC".to_string(),
            leg1_odds: 2.2,
            leg1_received_at_ms: 1,
            leg1_venue_market_id: "soft:1".to_string(),
            leg2_venue: leg2_venue.to_string(),
            leg2_selection: "SEABRA".to_string(),
            leg2_odds: 1.9,
            leg2_received_at_ms: 1,
            leg2_venue_market_id: "pin:1".to_string(),
            sigma: 0.98,
            arb_pct: 0.02,
            roi: 0.02,
            line: None,
            is_live: true,
        }
    }

    #[test]
    fn family_key_is_order_independent_for_pin_soft_pairs() {
        let a = candidate("draftkings", "pinnacle", "full_game");
        let b = candidate("pinnacle", "draftkings", "full_game");
        assert_eq!(family_key(&a), family_key(&b));
    }

    #[test]
    fn family_key_preserves_period() {
        let full_game = candidate("draftkings", "pinnacle", "full_game");
        let first_period = candidate("draftkings", "pinnacle", "first_period");
        assert_ne!(family_key(&full_game), family_key(&first_period));
    }
}
