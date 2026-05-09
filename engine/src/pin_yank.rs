use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone)]
struct YankEntry {
    expires_at_ms: u64,
    reason: String,
}

static BLACKLIST: OnceLock<Mutex<HashMap<String, YankEntry>>> = OnceLock::new();

fn blacklist() -> &'static Mutex<HashMap<String, YankEntry>> {
    BLACKLIST.get_or_init(|| Mutex::new(HashMap::new()))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn ttl_ms() -> u64 {
    std::env::var("PIN_YANK_BLACKLIST_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30 * 60 * 1000)
}

pub fn is_pin_hard_yank_error(message: &str) -> bool {
    let msg = message.to_ascii_lowercase();
    msg.contains("410")
        || msg.contains("offline")
        || msg.contains("market is unavailable")
        || msg.contains("market unavailable")
        || msg.contains("404")
        || msg.contains("matchup not found")
        || msg.contains("not_found")
        || msg.contains("selection unavailable")
        || msg.contains("suspended")
}

pub fn record_pin_yank(venue_market_id: &str, reason: &str) {
    let ttl = ttl_ms();
    if ttl == 0 || venue_market_id.trim().is_empty() || !is_pin_hard_yank_error(reason) {
        return;
    }

    let entry = YankEntry {
        expires_at_ms: now_ms().saturating_add(ttl),
        reason: reason.to_string(),
    };

    if let Ok(mut map) = blacklist().lock() {
        map.insert(venue_market_id.to_string(), entry);
    }
}

pub fn is_blacklisted(venue_market_id: &str) -> bool {
    if venue_market_id.trim().is_empty() {
        return false;
    }

    let now = now_ms();
    let Ok(mut map) = blacklist().lock() else {
        return false;
    };

    match map.get(venue_market_id) {
        Some(entry) if entry.expires_at_ms > now => {
            tracing::debug!(
                market = venue_market_id,
                reason = %entry.reason,
                "PIN market is on temporary hard-yank blacklist"
            );
            true
        }
        Some(_) => {
            map.remove(venue_market_id);
            false
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_hard_yank_errors() {
        assert!(is_pin_hard_yank_error("410 OFFLINE"));
        assert!(is_pin_hard_yank_error("404 Matchup not found"));
        assert!(is_pin_hard_yank_error("Market is unavailable"));
        assert!(!is_pin_hard_yank_error("temporary timeout"));
    }
}
