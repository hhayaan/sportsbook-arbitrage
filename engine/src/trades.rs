//! Structured trade event logging — all trade events go through here.

use crate::models::ArbCandidate;
use crate::state_machine::ArbAttempt;

pub fn log_arb_detected(candidate: &ArbCandidate, total_detected: u64) {
    tracing::info!(
        target: "trades",
        event = "ARB_DETECTED",
        event_id = %candidate.event_id,
        sport = %candidate.sport,
        sigma = candidate.sigma,
        arb_pct = candidate.arb_pct * 100.0,
        total_detected = total_detected,
        "Arb: {}/{} {:.3} vs {}/{} {:.3} | sigma={:.4} arb={:.2}%",
        candidate.leg1_venue, candidate.leg1_selection, candidate.leg1_odds,
        candidate.leg2_venue, candidate.leg2_selection, candidate.leg2_odds,
        candidate.sigma, candidate.arb_pct * 100.0
    );
}

pub fn log_sizing(candidate: &ArbCandidate, leg1_stake: f64, leg2_stake: f64, profit: f64) {
    tracing::info!(
        target: "trades",
        event = "SIZING",
        event_id = %candidate.event_id,
        leg1_stake = leg1_stake,
        leg2_stake = leg2_stake,
        profit = profit,
        "Sizing: S1=${:.2} on {}, S2=${:.2} on {}, profit=${:.2}",
        leg1_stake, candidate.leg1_venue, leg2_stake, candidate.leg2_venue, profit
    );
}

pub fn log_leg_filled(
    attempt_id: &str,
    leg: u8,
    venue: &str,
    selection: &str,
    stake: f64,
    odds: f64,
    status: &str,
) {
    tracing::info!(
        target: "trades",
        event = "LEG_FILL",
        attempt_id = %attempt_id,
        leg = leg,
        venue = %venue,
        status = %status,
        "Leg {} {}: ${:.2} on {} {} @ {:.3}",
        leg, status, stake, venue, selection, odds
    );
}

pub fn log_execution_closed(attempt: &ArbAttempt, profit: f64, total_traded: u64) {
    tracing::info!(
        target: "trades",
        event = "EXECUTION_CLOSED",
        attempt_id = %attempt.id,
        event_id = %attempt.event_id,
        profit = profit,
        total_traded = total_traded,
        "CLOSED: profit=${:.2} | {}/{} vs {}/{} | total: {}",
        profit, attempt.leg1_venue, attempt.leg1_selection,
        attempt.leg2_venue, attempt.leg2_selection, total_traded
    );
}

pub fn log_rescue_needed(
    attempt_id: &str,
    event_id: &str,
    leg2_status: &str,
    leg1_stake: f64,
    leg1_odds: f64,
) {
    tracing::warn!(
        target: "trades",
        event = "RESCUE_NEEDED",
        attempt_id = %attempt_id,
        event_id = %event_id,
        leg2_status = %leg2_status,
        exposure = leg1_stake,
        "Rescue needed: leg2 {} — exposure ${:.2} @ {:.3}",
        leg2_status, leg1_stake, leg1_odds
    );
}

pub fn log_rescue_computed(
    attempt_id: &str,
    venue: &str,
    selection: &str,
    stake: f64,
    odds: f64,
    worst_loss: f64,
) {
    tracing::info!(
        target: "trades",
        event = "RESCUE_COMPUTED",
        attempt_id = %attempt_id,
        rescue_venue = %venue,
        rescue_stake = stake,
        worst_case_loss = worst_loss,
        "Rescue: ${:.2} on {} {} @ {:.3} (worst: ${:.2})",
        stake, venue, selection, odds, worst_loss
    );
}

pub fn log_rescue_closed(attempt_id: &str, rescue_pnl: f64, venue: &str, stake: f64, odds: f64) {
    tracing::info!(
        target: "trades",
        event = "RESCUE_CLOSED",
        attempt_id = %attempt_id,
        rescue_pnl = rescue_pnl,
        "RESCUE CLOSED: P&L=${:.2} via {} @ {:.3}", rescue_pnl, venue, odds
    );
}

pub fn log_rescue_failed(attempt_id: &str, reason: &str, unhedged_exposure: f64) {
    tracing::error!(
        target: "trades",
        event = "RESCUE_FAILED",
        attempt_id = %attempt_id,
        reason = %reason,
        exposure = unhedged_exposure,
        "RESCUE FAILED: {} — exposure: ${:.2}", reason, unhedged_exposure
    );
}

pub fn log_gate_failure(event_id: &str, failures: &[String]) {
    tracing::info!(
        target: "trades",
        event = "POLICY_REJECTED",
        event_id = %event_id,
        "Gates failed: {:?}", failures
    );
}
