use crate::db::Db;
use crate::models::*;
use crate::stake_optimizer;
use std::sync::Arc;

pub fn try_execute(
    db: &Arc<Db>,
    candidate: &ArbCandidate,
    settings: &Settings,
) -> Option<ExecutionResult> {
    let min_arb_pct = settings.min_arb_pct / 100.0;
    if candidate.arb_pct < min_arb_pct {
        return None;
    }

    let leg1_balance = db.get_balance(&candidate.leg1_venue)?;
    let leg2_balance = db.get_balance(&candidate.leg2_venue)?;

    let requested_capital = stake_optimizer::requested_capital_with_soft_cap(
        candidate,
        settings,
        leg1_balance,
        leg2_balance,
    );

    if requested_capital <= 0.0 {
        return None;
    }

    let (plan, _effective_capital, _was_scaled) = stake_optimizer::compute_stakes_with_balances(
        candidate,
        requested_capital,
        leg1_balance,
        leg2_balance,
        settings.max_stake,
    )?;

    // Deduct balances
    db.update_balance(&candidate.leg1_venue, leg1_balance - plan.leg1_stake);
    db.update_balance(&candidate.leg2_venue, leg2_balance - plan.leg2_stake);

    // Add back stakes + profit (simulate instant settlement, split profit proportionally)
    let profit_share_1 = plan.guaranteed_profit * (plan.leg1_stake / plan.total_deployed);
    let profit_share_2 = plan.guaranteed_profit * (plan.leg2_stake / plan.total_deployed);
    db.update_balance(
        &candidate.leg1_venue,
        db.get_balance(&candidate.leg1_venue).unwrap_or(0.0) + plan.leg1_stake + profit_share_1,
    );
    db.update_balance(
        &candidate.leg2_venue,
        db.get_balance(&candidate.leg2_venue).unwrap_or(0.0) + plan.leg2_stake + profit_share_2,
    );

    let now = chrono::Utc::now().to_rfc3339();
    let trade = PaperTrade {
        id: uuid::Uuid::new_v4().to_string(),
        detected_at: now.clone(),
        executed_at: now,
        event_id: candidate.event_id.clone(),
        sport: candidate.sport.clone(),
        market_type: candidate.market_type.clone(),
        leg1_venue: candidate.leg1_venue.clone(),
        leg1_selection: candidate.leg1_selection.clone(),
        leg1_odds: candidate.leg1_odds,
        leg1_stake: plan.leg1_stake,
        leg2_venue: candidate.leg2_venue.clone(),
        leg2_selection: candidate.leg2_selection.clone(),
        leg2_odds: candidate.leg2_odds,
        leg2_stake: plan.leg2_stake,
        sigma: candidate.sigma,
        theoretical_profit: plan.guaranteed_profit,
        status: "executed".to_string(),
    };

    db.insert_trade(&trade);
    Some(ExecutionResult::trade(trade))
}
