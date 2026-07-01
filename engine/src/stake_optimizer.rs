use crate::models::{ArbCandidate, Settings, StakePlan};

/// Minimum capital to bother trading (both legs combined)
pub const MIN_CAPITAL: f64 = 1.0;
/// Minimum guaranteed profit to bother trading
pub const MIN_PROFIT: f64 = 0.01;

fn round_cents(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

pub fn requested_capital_with_soft_cap(
    candidate: &ArbCandidate,
    settings: &Settings,
    leg1_balance: f64,
    leg2_balance: f64,
) -> f64 {
    let target_stake = match settings.stake_mode.as_str() {
        "proportional" => {
            let soft_balance = if candidate.leg1_venue == "pinnacle" {
                leg2_balance
            } else if candidate.leg2_venue == "pinnacle" {
                leg1_balance
            } else {
                leg1_balance.min(leg2_balance)
            };
            soft_balance * settings.proportional_pct / 100.0
        }
        _ => settings.fixed_stake,
    }
    .min(settings.max_stake);

    if candidate.leg1_venue == "pinnacle" && candidate.leg2_venue != "pinnacle" {
        target_stake * candidate.sigma * candidate.leg2_odds
    } else if candidate.leg2_venue == "pinnacle" && candidate.leg1_venue != "pinnacle" {
        target_stake * candidate.sigma * candidate.leg1_odds
    } else {
        match settings.stake_mode.as_str() {
            "proportional" => {
                let smaller = leg1_balance.min(leg2_balance);
                (smaller * settings.proportional_pct / 100.0).min(settings.max_stake * 2.0)
            }
            _ => settings.fixed_stake * 2.0,
        }
    }
}

pub fn compute_stakes(candidate: &ArbCandidate, capital: f64) -> Option<StakePlan> {
    if candidate.sigma >= 1.0 || capital <= 0.0 {
        return None;
    }

    let s1 = capital * (1.0 / candidate.leg1_odds) / candidate.sigma;
    let s2 = capital * (1.0 / candidate.leg2_odds) / candidate.sigma;
    let guaranteed_payout = capital / candidate.sigma;
    let guaranteed_profit = guaranteed_payout - capital;

    if guaranteed_profit <= 0.0 {
        return None;
    }

    Some(StakePlan {
        leg1_stake: round_cents(s1),
        leg2_stake: round_cents(s2),
        total_deployed: round_cents(s1 + s2),
        guaranteed_payout: round_cents(guaranteed_payout),
        guaranteed_profit: round_cents(guaranteed_profit),
    })
}

/// Balance-aware stake scaling. Computes stakes from requested capital, then
/// scales down if either leg exceeds available balance or the soft-book stake cap.
/// Pinnacle hedge legs are not capped here; they are bounded later by the
/// authenticated quote, Pinnacle balance, and post-fill profitability checks.
/// Returns (StakePlan, effective_capital, was_scaled).
pub fn compute_stakes_with_balances(
    candidate: &ArbCandidate,
    requested_capital: f64,
    leg1_balance: f64,
    leg2_balance: f64,
    max_soft_stake: f64,
) -> Option<(StakePlan, f64, bool)> {
    if candidate.sigma >= 1.0 || requested_capital <= 0.0 {
        return None;
    }

    let s1 = requested_capital * (1.0 / candidate.leg1_odds) / candidate.sigma;
    let s2 = requested_capital * (1.0 / candidate.leg2_odds) / candidate.sigma;

    let mut effective_capital = requested_capital;
    let mut scaled = false;

    // Scale down if leg1 exceeds balance
    if s1 > leg1_balance {
        let max_cap = leg1_balance * candidate.sigma * candidate.leg1_odds;
        if max_cap < effective_capital {
            effective_capital = max_cap;
            scaled = true;
        }
    }

    // Scale down if leg2 exceeds balance
    if s2 > leg2_balance {
        let max_cap = leg2_balance * candidate.sigma * candidate.leg2_odds;
        if max_cap < effective_capital {
            effective_capital = max_cap;
            scaled = true;
        }
    }

    // Cap soft-book legs only. Pinnacle hedge sizing is quote/balance bounded.
    if candidate.leg1_venue != "pinnacle" {
        let max_cap_1 = max_soft_stake * candidate.sigma * candidate.leg1_odds;
        if max_cap_1 < effective_capital {
            effective_capital = max_cap_1;
            scaled = true;
        }
    }
    if candidate.leg2_venue != "pinnacle" {
        let max_cap_2 = max_soft_stake * candidate.sigma * candidate.leg2_odds;
        if max_cap_2 < effective_capital {
            effective_capital = max_cap_2;
            scaled = true;
        }
    }

    if effective_capital < MIN_CAPITAL {
        return None;
    }

    // Recompute at effective capital
    let s1 = effective_capital * (1.0 / candidate.leg1_odds) / candidate.sigma;
    let s2 = effective_capital * (1.0 / candidate.leg2_odds) / candidate.sigma;
    let guaranteed_payout = effective_capital / candidate.sigma;
    let guaranteed_profit = guaranteed_payout - effective_capital;

    if guaranteed_profit < MIN_PROFIT {
        return None;
    }

    Some((
        StakePlan {
            leg1_stake: round_cents(s1),
            leg2_stake: round_cents(s2),
            total_deployed: round_cents(s1 + s2),
            guaranteed_payout: round_cents(guaranteed_payout),
            guaranteed_profit: round_cents(guaranteed_profit),
        },
        round_cents(effective_capital),
        scaled,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(
        leg1_venue: &str,
        leg1_odds: f64,
        leg2_venue: &str,
        leg2_odds: f64,
    ) -> ArbCandidate {
        let sigma = 1.0 / leg1_odds + 1.0 / leg2_odds;
        ArbCandidate {
            event_id: "test-event".to_string(),
            sport: "MLB".to_string(),
            market_type: "moneyline".to_string(),
            surface_key: None,
            player_name: None,
            prop_type: None,
            period: "full_game".to_string(),
            start_time_utc: "2026-05-06T00:00:00Z".to_string(),
            leg1_venue: leg1_venue.to_string(),
            leg1_selection: "A".to_string(),
            leg1_odds,
            leg1_received_at_ms: 0,
            leg1_venue_market_id: "leg1".to_string(),
            leg2_venue: leg2_venue.to_string(),
            leg2_selection: "B".to_string(),
            leg2_odds,
            leg2_received_at_ms: 0,
            leg2_venue_market_id: "leg2".to_string(),
            sigma,
            arb_pct: 1.0 - sigma,
            roi: 0.0,
            line: None,
            is_live: false,
        }
    }

    fn settings(fixed_stake: f64, max_stake: f64) -> Settings {
        Settings {
            fixed_stake,
            max_stake,
            ..Settings::default()
        }
    }

    #[test]
    fn soft_cap_does_not_cap_pinnacle_hedge() {
        let c = candidate("draftkings", 6.0, "pinnacle", 1.35);
        let requested = requested_capital_with_soft_cap(&c, &settings(5.0, 5.0), 1000.0, 1000.0);
        let (plan, _, scaled) =
            compute_stakes_with_balances(&c, requested, 1000.0, 1000.0, 5.0).unwrap();

        assert!(!scaled);
        assert_eq!(plan.leg1_stake, 5.0);
        assert!(plan.leg2_stake > 10.0);
    }

    #[test]
    fn soft_cap_still_caps_non_pinnacle_pair() {
        let c = candidate("draftkings", 2.2, "fanduel", 2.2);
        let (plan, _, scaled) =
            compute_stakes_with_balances(&c, 40.0, 1000.0, 1000.0, 5.0).unwrap();

        assert!(scaled);
        assert!(plan.leg1_stake <= 5.0);
        assert!(plan.leg2_stake <= 5.0);
    }
}
