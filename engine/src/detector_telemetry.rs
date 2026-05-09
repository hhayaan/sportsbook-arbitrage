use crate::arb_detector::DetectionDiagnostics;
use crate::attempt_telemetry;
use crate::models::{ArbCandidate, CanonicalMarket};
use serde_json::json;
use std::collections::BTreeMap;

pub fn enabled() -> bool {
    env_bool("DETECTOR_TELEMETRY", false) || env_bool("DETECTOR_DIAGNOSTICS", false)
}

pub fn snapshot_limit() -> usize {
    std::env::var("DETECTOR_SNAPSHOT_LIMIT")
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .unwrap_or(50)
}

pub fn near_miss_sigma_max() -> f64 {
    std::env::var("DETECTOR_NEAR_MISS_SIGMA_MAX")
        .ok()
        .and_then(|raw| raw.trim().parse::<f64>().ok())
        .filter(|value| *value >= 1.0 && *value <= 2.0)
        .unwrap_or(1.02)
}

pub fn emit_market_counts(markets: &[CanonicalMarket]) {
    if !enabled() {
        return;
    }

    let mut counts: BTreeMap<String, MarketCount> = BTreeMap::new();
    for market in markets {
        let key = format!(
            "{}|{}|{}|{}",
            market.venue, market.sport, market.market_type, market.period
        );
        let entry = counts.entry(key).or_insert_with(|| MarketCount {
            venue: market.venue.clone(),
            sport: market.sport.clone(),
            market_type: market.market_type.clone(),
            period: market.period.clone(),
            count: 0,
        });
        entry.count += 1;
    }

    emit(
        "DETECTOR_MARKET_COUNTS",
        json!({ "counts": counts.into_values().collect::<Vec<_>>() }),
    );
}

pub fn emit_detection_diagnostics(diagnostics: &DetectionDiagnostics) {
    if !enabled() {
        return;
    }

    emit(
        "DETECTOR_FUNNEL_SUMMARY",
        json!({ "venue_pairs": diagnostics.venue_pairs }),
    );

    for snapshot in &diagnostics.snapshots {
        emit("DETECTOR_PIN_PAIR_SNAPSHOT", json!(snapshot));
    }
}

pub fn emit_candidate_events(candidates: &[ArbCandidate], min_arb_pct: f64) {
    if !enabled() {
        return;
    }

    let mut counts: BTreeMap<String, CandidateCount> = BTreeMap::new();
    for candidate in candidates {
        let (venue_a, venue_b) = venue_pair(&candidate.leg1_venue, &candidate.leg2_venue);
        let soft_venue_name = soft_venue(candidate).to_string();
        let key = format!(
            "{}|{}|{}|{}|{}|{}",
            venue_a,
            venue_b,
            soft_venue_name,
            candidate.sport,
            candidate.market_type,
            candidate.period
        );
        let entry = counts.entry(key).or_insert_with(|| CandidateCount {
            venue_pair: format!("{}/{}", venue_a, venue_b),
            soft_venue: soft_venue_name.clone(),
            sport: candidate.sport.clone(),
            market_type: candidate.market_type.clone(),
            period: candidate.period.clone(),
            detected: 0,
            passed_min_edge: 0,
        });
        entry.detected += 1;
        if candidate.arb_pct >= min_arb_pct {
            entry.passed_min_edge += 1;
        }

        emit(
            "DETECTOR_CANDIDATE",
            json!({
                "venue_pair": format!("{}/{}", venue_a, venue_b),
                "soft_venue": soft_venue(candidate),
                "event_id": candidate.event_id,
                "sport": candidate.sport,
                "market_type": candidate.market_type,
                "period": candidate.period,
                "line": candidate.line,
                "sigma": candidate.sigma,
                "arb_pct": candidate.arb_pct,
                "passes_min_edge": candidate.arb_pct >= min_arb_pct,
                "leg1": {
                    "venue": candidate.leg1_venue,
                    "selection": candidate.leg1_selection,
                    "odds": candidate.leg1_odds,
                    "venue_market_id": candidate.leg1_venue_market_id,
                },
                "leg2": {
                    "venue": candidate.leg2_venue,
                    "selection": candidate.leg2_selection,
                    "odds": candidate.leg2_odds,
                    "venue_market_id": candidate.leg2_venue_market_id,
                }
            }),
        );
    }

    emit(
        "DETECTOR_CANDIDATE_SUMMARY",
        json!({ "min_arb_pct": min_arb_pct, "counts": counts.into_values().collect::<Vec<_>>() }),
    );
}

fn emit(event_type: &str, payload: serde_json::Value) {
    if let Err(e) = attempt_telemetry::emit_event(event_type, None, payload) {
        tracing::debug!("detector telemetry write failed: {}", e);
    }
}

fn env_bool(name: &str, default: bool) -> bool {
    match std::env::var(name) {
        Ok(raw) => matches!(
            raw.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        Err(_) => default,
    }
}

fn venue_pair(a: &str, b: &str) -> (String, String) {
    if a <= b {
        (a.to_string(), b.to_string())
    } else {
        (b.to_string(), a.to_string())
    }
}

fn soft_venue(candidate: &ArbCandidate) -> &str {
    if candidate.leg1_venue == "pinnacle" {
        &candidate.leg2_venue
    } else {
        &candidate.leg1_venue
    }
}

#[derive(serde::Serialize)]
struct MarketCount {
    venue: String,
    sport: String,
    market_type: String,
    period: String,
    count: u64,
}

#[derive(serde::Serialize)]
struct CandidateCount {
    venue_pair: String,
    soft_venue: String,
    sport: String,
    market_type: String,
    period: String,
    detected: u64,
    passed_min_edge: u64,
}
