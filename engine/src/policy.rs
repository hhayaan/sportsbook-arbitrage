use crate::models::{ArbCandidate, StakePlan};
use std::collections::HashMap;

/// Check all policy gates. Returns ALL failure reasons (not just first).
pub fn check_gates(
    candidate: &ArbCandidate,
    plan: &StakePlan,
    now_ms: u64,
    max_data_age_ms: u64,
    min_stake: f64,
    min_profit: f64,
    min_seconds_to_start: i64,
    venue_balances: &HashMap<String, f64>,
    venue_last_seen_ms: Option<&HashMap<String, u64>>,
    source_health_threshold_ms: u64,
) -> Vec<String> {
    let mut failures = Vec::new();

    // Gate 1: Event too close (skip for live markets)
    if !candidate.is_live {
        if let Ok(start) = chrono::DateTime::parse_from_rfc3339(&candidate.start_time_utc) {
            let now = chrono::Utc::now();
            let seconds_to_start = (start.timestamp() - now.timestamp()).max(0);
            if seconds_to_start < min_seconds_to_start {
                failures.push(format!(
                    "event_too_close: {}s < {}s minimum",
                    seconds_to_start, min_seconds_to_start
                ));
            }
        }
    }

    // Gate 2: Data age
    let age1 = now_ms.saturating_sub(candidate.leg1_received_at_ms);
    if age1 > max_data_age_ms {
        failures.push(format!(
            "data_age: {} {}ms > {}ms",
            candidate.leg1_venue, age1, max_data_age_ms
        ));
    }
    let age2 = now_ms.saturating_sub(candidate.leg2_received_at_ms);
    if age2 > max_data_age_ms {
        failures.push(format!(
            "data_age: {} {}ms > {}ms",
            candidate.leg2_venue, age2, max_data_age_ms
        ));
    }

    // Gate 3: Min stake
    if plan.leg1_stake < min_stake || plan.leg2_stake < min_stake {
        failures.push(format!(
            "min_stake: ${:.2} < ${:.2}",
            plan.leg1_stake.min(plan.leg2_stake), min_stake
        ));
    }

    // Gate 4: Min profit
    if plan.guaranteed_profit < min_profit {
        failures.push(format!(
            "min_profit: ${:.2} < ${:.2}",
            plan.guaranteed_profit, min_profit
        ));
    }

    // Gate 5: Venue balance checks
    if let Some(&balance) = venue_balances.get(&candidate.leg1_venue) {
        if balance < plan.leg1_stake {
            failures.push(format!(
                "balance: {} has ${:.2}, needs ${:.2}",
                candidate.leg1_venue, balance, plan.leg1_stake
            ));
        }
    }
    if let Some(&balance) = venue_balances.get(&candidate.leg2_venue) {
        if balance < plan.leg2_stake {
            failures.push(format!(
                "balance: {} has ${:.2}, needs ${:.2}",
                candidate.leg2_venue, balance, plan.leg2_stake
            ));
        }
    }

    // Gate 6: Source health
    if let Some(last_seen) = venue_last_seen_ms {
        if source_health_threshold_ms > 0 {
            for venue_name in [&candidate.leg1_venue, &candidate.leg2_venue] {
                match last_seen.get(venue_name) {
                    Some(&last_ms) => {
                        let age = now_ms.saturating_sub(last_ms);
                        if age > source_health_threshold_ms {
                            failures.push(format!(
                                "source_unhealthy: {} no data for {}ms",
                                venue_name, age
                            ));
                        }
                    }
                    None => {
                        failures.push(format!(
                            "source_unhealthy: {} never received data",
                            venue_name
                        ));
                    }
                }
            }
        }
    }

    failures
}
