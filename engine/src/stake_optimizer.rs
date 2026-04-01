use crate::models::{ArbCandidate, StakePlan};

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
