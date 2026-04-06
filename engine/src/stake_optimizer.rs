use crate::models::{ArbCandidate, StakePlan};

/// Minimum capital to bother trading (both legs combined)
pub const MIN_CAPITAL: f64 = 1.0;
/// Minimum guaranteed profit to bother trading
pub const MIN_PROFIT: f64 = 0.01;

fn round_cents(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
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
/// scales down if either leg exceeds available balance or max_stake_per_leg.
/// Returns (StakePlan, effective_capital, was_scaled).
pub fn compute_stakes_with_balances(
    candidate: &ArbCandidate,
    requested_capital: f64,
    leg1_balance: f64,
    leg2_balance: f64,
    max_stake_per_leg: f64,
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

    // Cap by max_stake_per_leg
    let max_cap_1 = max_stake_per_leg * candidate.sigma * candidate.leg1_odds;
    let max_cap_2 = max_stake_per_leg * candidate.sigma * candidate.leg2_odds;
    if max_cap_1 < effective_capital {
        effective_capital = max_cap_1;
        scaled = true;
    }
    if max_cap_2 < effective_capital {
        effective_capital = max_cap_2;
        scaled = true;
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
