use crate::models::ArbCandidate;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

const DEFAULT_TTL_MS: u64 = 6 * 60 * 60 * 1000;
const DEFAULT_SUCCESS_COOLDOWN_MS: u64 = 30 * 1000;
const DEFAULT_SUCCESS_MAX_REPEATS: u32 = 3;

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
    #[serde(default)]
    pub hard_lock: bool,
    #[serde(default)]
    pub successful_fills: u32,
    #[serde(default)]
    pub cooldown_until_ms: u64,
}

#[derive(Debug, Clone)]
pub struct FamilyDedupHit {
    pub key: String,
    pub reason: String,
    pub remaining_secs: u64,
    pub policy: String,
    pub successful_fills: u32,
    pub max_successful_fills: u32,
}

pub struct FamilyDedup {
    entries: HashMap<String, FamilyDedupEntry>,
    path: PathBuf,
    ttl_ms: u64,
    success_cooldown_ms: u64,
    success_max_repeats: u32,
}

impl FamilyDedup {
    pub fn ttl_secs_from_env() -> u64 {
        ttl_ms_from_env() / 1000
    }

    pub fn success_cooldown_secs_from_env() -> u64 {
        success_cooldown_ms_from_env() / 1000
    }

    pub fn success_max_repeats_from_env() -> u32 {
        success_max_repeats_from_env()
    }

    pub fn load_from_env() -> Self {
        let path = std::env::var("FAMILY_DEDUP_FILE")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("data/family-dedup.json"));
        let ttl_ms = ttl_ms_from_env();
        let success_cooldown_ms = success_cooldown_ms_from_env();
        let success_max_repeats = success_max_repeats_from_env();
        let mut dedup = Self {
            entries: HashMap::new(),
            path,
            ttl_ms,
            success_cooldown_ms,
            success_max_repeats,
        };
        dedup.load();
        dedup.prune_expired();
        dedup
    }

    pub fn ttl_secs(&self) -> u64 {
        self.ttl_ms / 1000
    }

    pub fn success_cooldown_secs(&self) -> u64 {
        self.success_cooldown_ms / 1000
    }

    pub fn success_max_repeats(&self) -> u32 {
        self.success_max_repeats
    }

    pub fn max_successful_fills(&self) -> u32 {
        self.success_max_repeats.saturating_add(1)
    }

    pub fn lock_status(&mut self, candidate: &ArbCandidate) -> Option<FamilyDedupHit> {
        self.prune_expired();
        let key = family_key(candidate);
        let now = now_ms();
        self.entries.get(&key).and_then(|entry| {
            if entry.expires_at_ms <= now {
                None
            } else if is_success_entry(entry) {
                let successful_fills = entry.successful_fills.max(1);
                let max_successful_fills = self.max_successful_fills();
                if successful_fills >= max_successful_fills {
                    Some(FamilyDedupHit {
                        key,
                        reason: "success_repeat_limit".to_string(),
                        remaining_secs: ((entry.expires_at_ms - now) + 999) / 1000,
                        policy: "success_repeat_limit".to_string(),
                        successful_fills,
                        max_successful_fills,
                    })
                } else if entry.cooldown_until_ms > now {
                    Some(FamilyDedupHit {
                        key,
                        reason: "success_cooldown".to_string(),
                        remaining_secs: ((entry.cooldown_until_ms - now) + 999) / 1000,
                        policy: "success_cooldown".to_string(),
                        successful_fills,
                        max_successful_fills,
                    })
                } else {
                    None
                }
            } else {
                Some(FamilyDedupHit {
                    key,
                    reason: entry.reason.clone(),
                    remaining_secs: ((entry.expires_at_ms - now) + 999) / 1000,
                    policy: "hard_lock".to_string(),
                    successful_fills: entry.successful_fills,
                    max_successful_fills: self.max_successful_fills(),
                })
            }
        })
    }

    pub fn record_hard_lock(
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
            hard_lock: true,
            successful_fills: 0,
            cooldown_until_ms: 0,
        };
        self.entries.insert(key, entry.clone());
        self.save();
        entry
    }

    pub fn record_successful_fill(&mut self, candidate: &ArbCandidate) -> FamilyDedupEntry {
        let now = now_ms();
        let key = family_key(candidate);
        let previous_successful_fills = self
            .entries
            .get(&key)
            .filter(|entry| is_success_entry(entry))
            .map(|entry| entry.successful_fills.max(1))
            .unwrap_or(0);
        let entry = FamilyDedupEntry {
            key: key.clone(),
            sport: candidate.sport.clone(),
            event_id: candidate.event_id.clone(),
            soft_venue: soft_venue(candidate),
            market_type: candidate.market_type.clone(),
            period: period_label(candidate),
            reason: "both_filled".to_string(),
            locked_at_ms: now,
            expires_at_ms: now + self.ttl_ms,
            hard_lock: false,
            successful_fills: previous_successful_fills.saturating_add(1),
            cooldown_until_ms: now + self.success_cooldown_ms,
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
        let mut dropped_obsolete = 0usize;
        self.entries = entries
            .into_iter()
            .filter_map(|mut entry| {
                if is_success_entry(&entry) {
                    entry.hard_lock = false;
                    entry.successful_fills = entry.successful_fills.max(1);
                    if entry.cooldown_until_ms == 0 {
                        entry.cooldown_until_ms = entry.locked_at_ms + self.success_cooldown_ms;
                    }
                } else if is_obsolete_broad_failure_lock(&entry.reason) {
                    dropped_obsolete += 1;
                    return None;
                } else {
                    entry.hard_lock = true;
                }
                Some((entry.key.clone(), entry))
            })
            .collect();
        if dropped_obsolete > 0 {
            tracing::info!(
                "[FAMILY DEDUP] Dropped {} obsolete broad failure lock(s)",
                dropped_obsolete
            );
            self.save();
        }
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

fn is_success_entry(entry: &FamilyDedupEntry) -> bool {
    !entry.hard_lock && (entry.reason == "both_filled" || entry.successful_fills > 0)
}

fn is_obsolete_broad_failure_lock(reason: &str) -> bool {
    let lower = reason.to_ascii_lowercase();
    lower.ends_with("_purchase_not_accepted")
        || lower.ends_with("_declined")
        || lower.ends_with("_nonretryable_leg1_failure")
}

pub fn family_key(candidate: &ArbCandidate) -> String {
    format!(
        "{}|{}|{}|{}|{}|{}|{}|{}",
        candidate.sport.to_ascii_lowercase(),
        candidate.event_id.to_ascii_lowercase(),
        soft_venue(candidate),
        candidate.market_type.to_ascii_lowercase(),
        period_label(candidate).to_ascii_lowercase(),
        candidate
            .surface_key
            .clone()
            .unwrap_or_default()
            .to_ascii_lowercase(),
        candidate
            .player_name
            .clone()
            .unwrap_or_default()
            .to_ascii_lowercase(),
        candidate
            .prop_type
            .clone()
            .unwrap_or_default()
            .to_ascii_lowercase(),
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

fn success_cooldown_ms_from_env() -> u64 {
    if let Some(v) = env_u64("FAMILY_DEDUP_SUCCESS_COOLDOWN_MS") {
        return v;
    }
    if let Some(v) = env_u64("FAMILY_DEDUP_SUCCESS_COOLDOWN_SECS") {
        return v.saturating_mul(1000);
    }
    DEFAULT_SUCCESS_COOLDOWN_MS
}

fn success_max_repeats_from_env() -> u32 {
    std::env::var("FAMILY_DEDUP_SUCCESS_MAX_REPEATS")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(DEFAULT_SUCCESS_MAX_REPEATS)
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
    use super::{family_key, FamilyDedup};
    use crate::models::ArbCandidate;

    fn candidate(leg1_venue: &str, leg2_venue: &str, period: &str) -> ArbCandidate {
        ArbCandidate {
            event_id: "UFC:2026-05-09T17:JOABUC-vs-SEABRA".to_string(),
            sport: "UFC".to_string(),
            market_type: "moneyline".to_string(),
            surface_key: None,
            player_name: None,
            prop_type: None,
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

    fn dedup_for_test(ttl_ms: u64, cooldown_ms: u64, max_repeats: u32) -> FamilyDedup {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        FamilyDedup {
            entries: Default::default(),
            path: std::env::temp_dir().join(format!(
                "test-family-dedup-{}-{}.json",
                std::process::id(),
                nonce
            )),
            ttl_ms,
            success_cooldown_ms: cooldown_ms,
            success_max_repeats: max_repeats,
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

    #[test]
    fn successful_fill_uses_cooldown_not_hard_lock() {
        let mut dedup = dedup_for_test(10_000, 30_000, 3);
        let candidate = candidate("draftkings", "pinnacle", "full_game");

        dedup.record_successful_fill(&candidate);

        let hit = dedup
            .lock_status(&candidate)
            .expect("cooldown should block");
        assert_eq!(hit.policy, "success_cooldown");
        assert_eq!(hit.successful_fills, 1);
        assert_eq!(hit.max_successful_fills, 4);
    }

    #[test]
    fn successful_fill_blocks_after_repeat_limit() {
        let mut dedup = dedup_for_test(10_000, 0, 3);
        let candidate = candidate("draftkings", "pinnacle", "full_game");

        for _ in 0..4 {
            dedup.record_successful_fill(&candidate);
        }

        let hit = dedup
            .lock_status(&candidate)
            .expect("repeat limit should block");
        assert_eq!(hit.policy, "success_repeat_limit");
        assert_eq!(hit.successful_fills, 4);
        assert_eq!(hit.max_successful_fills, 4);
    }

    #[test]
    fn hard_lock_blocks_immediately() {
        let mut dedup = dedup_for_test(10_000, 0, 3);
        let candidate = candidate("draftkings", "pinnacle", "full_game");

        dedup.record_hard_lock(&candidate, "draftkings_max_exposure_reached");

        let hit = dedup
            .lock_status(&candidate)
            .expect("hard lock should block");
        assert_eq!(hit.policy, "hard_lock");
        assert_eq!(hit.reason, "draftkings_max_exposure_reached");
    }

    #[test]
    fn load_drops_obsolete_broad_failure_locks() {
        let mut dedup = dedup_for_test(10_000, 0, 3);
        let candidate = candidate("draftkings", "pinnacle", "full_game");
        let raw = format!(
            r#"[{{
  "key": "{}",
  "sport": "UFC",
  "event_id": "UFC:2026-05-09T17:JOABUC-vs-SEABRA",
  "soft_venue": "draftkings",
  "market_type": "moneyline",
  "period": "full_game",
  "reason": "draftkings_purchase_not_accepted",
  "locked_at_ms": 1,
  "expires_at_ms": 9999999999999
}}]"#,
            family_key(&candidate)
        );
        std::fs::write(&dedup.path, raw).expect("test dedup file should write");

        dedup.load();

        assert!(dedup.lock_status(&candidate).is_none());
    }
}
