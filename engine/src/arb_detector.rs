use crate::models::*;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};

/// Sports with 3-way moneyline (home/draw/away). 2-leg arbs are invalid here.
const THREE_WAY_SPORTS: &[&str] = &[
    "SOCCER",
    "EPL",
    "LA_LIGA",
    "SERIE_A",
    "BUNDESLIGA",
    "LIGUE_1",
    "UCL",
    "MLS",
];

#[derive(Debug, Clone, Serialize, Default)]
pub struct DetectionDiagnostics {
    pub venue_pairs: Vec<VenuePairDiagnostics>,
    pub snapshots: Vec<DetectorSnapshot>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct VenuePairDiagnostics {
    pub venue_pair: String,
    pub venue_a: String,
    pub venue_b: String,
    pub sport: String,
    pub market_type: String,
    pub line_key: String,
    pub period: String,
    pub shared_identity_keys: u64,
    pub selection_pair_checks: u64,
    pub complementary_selection_pairs: u64,
    pub rejected_three_way_moneyline: u64,
    pub rejected_partial_period_moneyline: u64,
    pub rejected_spread_null_line: u64,
    pub rejected_spread_same_sign: u64,
    pub rejected_stale: u64,
    pub rejected_no_pinnacle: u64,
    pub sigma_not_arb: u64,
    pub near_miss: u64,
    pub candidates: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DetectorSnapshot {
    pub reason: String,
    pub venue_pair: String,
    pub event_id: String,
    pub sport: String,
    pub market_type: String,
    pub line_key: String,
    pub period: String,
    pub sigma: f64,
    pub leg_a: DetectorSnapshotLeg,
    pub leg_b: DetectorSnapshotLeg,
}

#[derive(Debug, Clone, Serialize)]
pub struct DetectorSnapshotLeg {
    pub venue: String,
    pub selection: String,
    pub odds: f64,
    pub line: Option<f64>,
    pub venue_market_id: String,
    pub venue_event_id: String,
    pub received_at_ms: u64,
}

pub struct DetectionReport {
    pub candidates: Vec<ArbCandidate>,
    pub diagnostics: DetectionDiagnostics,
}

pub fn detect_arbs(
    book: &Book,
    now_ms: u64,
    max_data_age_ms: u64,
    require_pinnacle_leg: bool,
) -> Vec<ArbCandidate> {
    let mut candidates = Vec::new();

    for key in book.identity_keys() {
        let entries = match book.get_entries(&key) {
            Some(e) => e,
            None => continue,
        };

        let mut by_selection: HashMap<String, Vec<&PriceEntry>> = HashMap::new();
        for entry in entries.values() {
            by_selection
                .entry(entry.selection.clone())
                .or_default()
                .push(entry);
        }

        let selections: Vec<String> = by_selection.keys().cloned().collect();
        if selections.len() < 2 {
            continue;
        }

        let sport = key.event_id.split(':').next().unwrap_or("");
        let is_three_way_sport = THREE_WAY_SPORTS.iter().any(|&s| s == sport);
        if is_three_way_sport && key.market_type == "moneyline" {
            continue;
        }

        if key.market_type == "moneyline" && key.period != "full_game" {
            continue;
        }

        for i in 0..selections.len() {
            for j in (i + 1)..selections.len() {
                let sel_a = &selections[i];
                let sel_b = &selections[j];

                if !are_selections_complementary(&key.market_type, sel_a, sel_b) {
                    continue;
                }

                let entries_a = &by_selection[sel_a];
                let entries_b = &by_selection[sel_b];

                for ea in entries_a {
                    for eb in entries_b {
                        if ea.venue == eb.venue {
                            continue;
                        }

                        if key.market_type == "spread" || key.market_type == "alt_spread" {
                            match (ea.line, eb.line) {
                                (Some(la), Some(lb)) => {
                                    if la.signum() == lb.signum() {
                                        continue;
                                    }
                                }
                                _ => {
                                    tracing::warn!(
                                        "[SPREAD NULL LINE] {}:{} line={:?} vs {}:{} line={:?} | event={}",
                                        ea.venue,
                                        ea.selection,
                                        ea.line,
                                        eb.venue,
                                        eb.selection,
                                        eb.line,
                                        key.event_id
                                    );
                                    continue;
                                }
                            }
                        }

                        let age_a = now_ms.saturating_sub(ea.received_at_ms);
                        let age_b = now_ms.saturating_sub(eb.received_at_ms);
                        if age_a > max_data_age_ms || age_b > max_data_age_ms {
                            continue;
                        }

                        let sigma = 1.0 / ea.decimal_odds + 1.0 / eb.decimal_odds;

                        if require_pinnacle_leg {
                            let has_pin = ea.venue == "pinnacle" || eb.venue == "pinnacle";
                            if !has_pin {
                                if sigma < 1.0 {
                                    tracing::info!(
                                        "[SKIP] {}<->{} no Pinnacle leg: {} sigma={:.4} arb={:.2}%",
                                        ea.venue,
                                        eb.venue,
                                        key.event_id,
                                        sigma,
                                        (1.0 - sigma) * 100.0
                                    );
                                }
                                continue;
                            }
                        }

                        if sigma >= 1.0 && sigma < 1.02 {
                            tracing::debug!(
                                "[near-miss] sigma={:.4} {} {}@{:.3} ({}) vs {}@{:.3} ({})",
                                sigma,
                                key.event_id,
                                ea.selection,
                                ea.decimal_odds,
                                ea.venue,
                                eb.selection,
                                eb.decimal_odds,
                                eb.venue,
                            );
                        }

                        if sigma < 1.0 {
                            let arb_pct = 1.0 - sigma;
                            let roi = 1.0 / sigma - 1.0;

                            candidates.push(ArbCandidate {
                                event_id: key.event_id.clone(),
                                sport: ea.sport.clone(),
                                market_type: key.market_type.clone(),
                                period: key.period.clone(),
                                start_time_utc: ea.start_time_utc.clone(),
                                leg1_venue: ea.venue.clone(),
                                leg1_selection: ea.selection.clone(),
                                leg1_odds: ea.decimal_odds,
                                leg1_received_at_ms: ea.received_at_ms,
                                leg1_venue_market_id: ea.venue_market_id.clone(),
                                leg2_venue: eb.venue.clone(),
                                leg2_selection: eb.selection.clone(),
                                leg2_odds: eb.decimal_odds,
                                leg2_received_at_ms: eb.received_at_ms,
                                leg2_venue_market_id: eb.venue_market_id.clone(),
                                sigma,
                                arb_pct,
                                roi,
                                line: ea.line,
                                is_live: ea.is_live || eb.is_live,
                            });
                        }
                    }
                }
            }
        }
    }

    candidates.sort_by(|a, b| {
        b.arb_pct
            .partial_cmp(&a.arb_pct)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    candidates
}

pub fn analyze(
    book: &Book,
    now_ms: u64,
    max_data_age_ms: u64,
    require_pinnacle_leg: bool,
    snapshot_limit: usize,
    near_miss_sigma_max: f64,
) -> DetectionReport {
    let mut candidates = Vec::new();
    let mut counters: BTreeMap<String, VenuePairDiagnostics> = BTreeMap::new();
    let mut snapshots = Vec::new();

    for key in book.identity_keys() {
        let entries = match book.get_entries(&key) {
            Some(e) => e,
            None => continue,
        };
        let sport = key.event_id.split(':').next().unwrap_or("").to_string();

        for (venue_a, venue_b) in venue_pairs_for_entries(entries.values()) {
            counter_for(&mut counters, &key, &sport, &venue_a, &venue_b).shared_identity_keys += 1;
        }

        let mut by_selection: HashMap<String, Vec<&PriceEntry>> = HashMap::new();
        for entry in entries.values() {
            by_selection
                .entry(entry.selection.clone())
                .or_default()
                .push(entry);
        }

        let selections: Vec<String> = by_selection.keys().cloned().collect();
        if selections.len() < 2 {
            continue;
        }

        let is_three_way_sport = THREE_WAY_SPORTS.iter().any(|&s| s == sport);
        if is_three_way_sport && key.market_type == "moneyline" {
            for (venue_a, venue_b) in venue_pairs_for_entries(entries.values()) {
                counter_for(&mut counters, &key, &sport, &venue_a, &venue_b)
                    .rejected_three_way_moneyline += 1;
            }
            continue;
        }

        if key.market_type == "moneyline" && key.period != "full_game" {
            for (venue_a, venue_b) in venue_pairs_for_entries(entries.values()) {
                counter_for(&mut counters, &key, &sport, &venue_a, &venue_b)
                    .rejected_partial_period_moneyline += 1;
            }
            continue;
        }

        for i in 0..selections.len() {
            for j in (i + 1)..selections.len() {
                let sel_a = &selections[i];
                let sel_b = &selections[j];

                if !are_selections_complementary(&key.market_type, sel_a, sel_b) {
                    continue;
                }

                let entries_a = &by_selection[sel_a];
                let entries_b = &by_selection[sel_b];

                for ea in entries_a {
                    for eb in entries_b {
                        if ea.venue == eb.venue {
                            continue;
                        }

                        let (venue_a, venue_b) = venue_pair(&ea.venue, &eb.venue);
                        let pair_counter =
                            counter_for(&mut counters, &key, &sport, &venue_a, &venue_b);
                        pair_counter.selection_pair_checks += 1;
                        pair_counter.complementary_selection_pairs += 1;

                        if key.market_type == "spread" || key.market_type == "alt_spread" {
                            match (ea.line, eb.line) {
                                (Some(la), Some(lb)) => {
                                    if la.signum() == lb.signum() {
                                        pair_counter.rejected_spread_same_sign += 1;
                                        continue;
                                    }
                                }
                                _ => {
                                    pair_counter.rejected_spread_null_line += 1;
                                    tracing::warn!(
                                        "[SPREAD NULL LINE] {}:{} line={:?} vs {}:{} line={:?} | event={}",
                                        ea.venue,
                                        ea.selection,
                                        ea.line,
                                        eb.venue,
                                        eb.selection,
                                        eb.line,
                                        key.event_id
                                    );
                                    continue;
                                }
                            }
                        }

                        let age_a = now_ms.saturating_sub(ea.received_at_ms);
                        let age_b = now_ms.saturating_sub(eb.received_at_ms);
                        if age_a > max_data_age_ms || age_b > max_data_age_ms {
                            pair_counter.rejected_stale += 1;
                            continue;
                        }

                        let sigma = 1.0 / ea.decimal_odds + 1.0 / eb.decimal_odds;

                        if require_pinnacle_leg {
                            let has_pin = ea.venue == "pinnacle" || eb.venue == "pinnacle";
                            if !has_pin {
                                pair_counter.rejected_no_pinnacle += 1;
                                if sigma < 1.0 {
                                    tracing::info!(
                                        "[SKIP] {}<->{} no Pinnacle leg: {} sigma={:.4} arb={:.2}%",
                                        ea.venue,
                                        eb.venue,
                                        key.event_id,
                                        sigma,
                                        (1.0 - sigma) * 100.0
                                    );
                                }
                                continue;
                            }
                        }

                        if sigma >= 1.0 {
                            pair_counter.sigma_not_arb += 1;
                            if sigma < 1.02 {
                                pair_counter.near_miss += 1;
                                tracing::debug!(
                                    "[near-miss] sigma={:.4} {} {}@{:.3} ({}) vs {}@{:.3} ({})",
                                    sigma,
                                    key.event_id,
                                    ea.selection,
                                    ea.decimal_odds,
                                    ea.venue,
                                    eb.selection,
                                    eb.decimal_odds,
                                    eb.venue,
                                );
                            }
                            if sigma <= near_miss_sigma_max {
                                maybe_push_snapshot(
                                    &mut snapshots,
                                    snapshot_limit,
                                    "near_miss",
                                    &key,
                                    &sport,
                                    sigma,
                                    ea,
                                    eb,
                                );
                            }
                            continue;
                        }

                        pair_counter.candidates += 1;
                        maybe_push_snapshot(
                            &mut snapshots,
                            snapshot_limit,
                            "candidate",
                            &key,
                            &sport,
                            sigma,
                            ea,
                            eb,
                        );

                        let arb_pct = 1.0 - sigma;
                        let roi = 1.0 / sigma - 1.0;
                        candidates.push(ArbCandidate {
                            event_id: key.event_id.clone(),
                            sport: ea.sport.clone(),
                            market_type: key.market_type.clone(),
                            period: key.period.clone(),
                            start_time_utc: ea.start_time_utc.clone(),
                            leg1_venue: ea.venue.clone(),
                            leg1_selection: ea.selection.clone(),
                            leg1_odds: ea.decimal_odds,
                            leg1_received_at_ms: ea.received_at_ms,
                            leg1_venue_market_id: ea.venue_market_id.clone(),
                            leg2_venue: eb.venue.clone(),
                            leg2_selection: eb.selection.clone(),
                            leg2_odds: eb.decimal_odds,
                            leg2_received_at_ms: eb.received_at_ms,
                            leg2_venue_market_id: eb.venue_market_id.clone(),
                            sigma,
                            arb_pct,
                            roi,
                            line: ea.line,
                            is_live: ea.is_live || eb.is_live,
                        });
                    }
                }
            }
        }
    }

    candidates.sort_by(|a, b| {
        b.arb_pct
            .partial_cmp(&a.arb_pct)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    DetectionReport {
        candidates,
        diagnostics: DetectionDiagnostics {
            venue_pairs: counters.into_values().collect(),
            snapshots,
        },
    }
}

fn venue_pairs_for_entries<'a>(
    entries: impl Iterator<Item = &'a PriceEntry>,
) -> Vec<(String, String)> {
    let venues: Vec<String> = entries
        .map(|entry| entry.venue.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    let mut pairs = Vec::new();
    for i in 0..venues.len() {
        for j in (i + 1)..venues.len() {
            pairs.push(venue_pair(&venues[i], &venues[j]));
        }
    }
    pairs.sort();
    pairs.dedup();
    pairs
}

fn counter_for<'a>(
    counters: &'a mut BTreeMap<String, VenuePairDiagnostics>,
    key: &MarketIdentityKey,
    sport: &str,
    venue_a: &str,
    venue_b: &str,
) -> &'a mut VenuePairDiagnostics {
    let map_key = format!(
        "{}|{}|{}|{}|{}|{}",
        venue_a, venue_b, sport, key.market_type, key.period, key.line_key
    );
    counters
        .entry(map_key)
        .or_insert_with(|| VenuePairDiagnostics {
            venue_pair: format!("{}/{}", venue_a, venue_b),
            venue_a: venue_a.to_string(),
            venue_b: venue_b.to_string(),
            sport: sport.to_string(),
            market_type: key.market_type.clone(),
            line_key: key.line_key.clone(),
            period: key.period.clone(),
            ..Default::default()
        })
}

fn venue_pair(a: &str, b: &str) -> (String, String) {
    if a <= b {
        (a.to_string(), b.to_string())
    } else {
        (b.to_string(), a.to_string())
    }
}

fn maybe_push_snapshot(
    snapshots: &mut Vec<DetectorSnapshot>,
    snapshot_limit: usize,
    reason: &str,
    key: &MarketIdentityKey,
    sport: &str,
    sigma: f64,
    ea: &PriceEntry,
    eb: &PriceEntry,
) {
    if snapshot_limit == 0 || snapshots.len() >= snapshot_limit {
        return;
    }
    if ea.venue != "pinnacle" && eb.venue != "pinnacle" {
        return;
    }
    let (venue_a, venue_b) = venue_pair(&ea.venue, &eb.venue);
    snapshots.push(DetectorSnapshot {
        reason: reason.to_string(),
        venue_pair: format!("{}/{}", venue_a, venue_b),
        event_id: key.event_id.clone(),
        sport: sport.to_string(),
        market_type: key.market_type.clone(),
        line_key: key.line_key.clone(),
        period: key.period.clone(),
        sigma,
        leg_a: snapshot_leg(ea),
        leg_b: snapshot_leg(eb),
    });
}

fn snapshot_leg(entry: &PriceEntry) -> DetectorSnapshotLeg {
    DetectorSnapshotLeg {
        venue: entry.venue.clone(),
        selection: entry.selection.clone(),
        odds: entry.decimal_odds,
        line: entry.line,
        venue_market_id: entry.venue_market_id.clone(),
        venue_event_id: entry.venue_event_id.clone(),
        received_at_ms: entry.received_at_ms,
    }
}

fn are_selections_complementary(market_type: &str, sel_a: &str, sel_b: &str) -> bool {
    match market_type {
        "moneyline" => sel_a != sel_b && sel_a != "draw" && sel_b != "draw",
        "total" | "team_total" | "alt_total" | "player_prop" => {
            (sel_a == "over" && sel_b == "under") || (sel_a == "under" && sel_b == "over")
        }
        "spread" | "alt_spread" => sel_a != sel_b,
        _ => false,
    }
}
