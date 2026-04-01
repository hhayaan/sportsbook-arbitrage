use crate::models::*;

pub fn detect_arbs(book: &Book, now_ms: u64, max_data_age_ms: u64, require_pinnacle_leg: bool) -> Vec<ArbCandidate> {
    let mut candidates = Vec::new();

    for key in book.identity_keys() {
        let entries = match book.get_entries(&key) {
            Some(e) => e,
            None => continue,
        };

        let mut by_selection: std::collections::HashMap<String, Vec<&PriceEntry>> =
            std::collections::HashMap::new();
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

                        let age_a = now_ms.saturating_sub(ea.received_at_ms);
                        let age_b = now_ms.saturating_sub(eb.received_at_ms);
                        if age_a > max_data_age_ms || age_b > max_data_age_ms {
                            continue;
                        }

                        let sigma = 1.0 / ea.decimal_odds + 1.0 / eb.decimal_odds;

                        // Pinnacle strategy filter
                        if require_pinnacle_leg {
                            let has_pin = ea.venue == "pinnacle" || eb.venue == "pinnacle";
                            if !has_pin {
                                if sigma < 1.0 {
                                    tracing::info!(
                                        "[SKIP] {}↔{} no Pinnacle leg: {} sigma={:.4} arb={:.2}%",
                                        ea.venue, eb.venue, key.event_id,
                                        sigma, (1.0 - sigma) * 100.0
                                    );
                                }
                                continue;
                            }
                        }

                        if sigma >= 1.0 && sigma < 1.02 {
                            tracing::debug!(
                                "[near-miss] sigma={:.4} {} {}@{:.3} ({}) vs {}@{:.3} ({})",
                                sigma, key.event_id,
                                ea.selection, ea.decimal_odds, ea.venue,
                                eb.selection, eb.decimal_odds, eb.venue,
                            );
                        }

                        if sigma < 1.0 {
                            let arb_pct = 1.0 - sigma;
                            let roi = 1.0 / sigma - 1.0;

                            candidates.push(ArbCandidate {
                                event_id: key.event_id.clone(),
                                sport: ea.sport.clone(),
                                market_type: key.market_type.clone(),
                                period: "full_game".to_string(),
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
                                is_live: key.is_live,
                            });
                        }
                    }
                }
            }
        }
    }

    candidates.sort_by(|a, b| b.arb_pct.partial_cmp(&a.arb_pct).unwrap_or(std::cmp::Ordering::Equal));
    candidates
}

fn are_selections_complementary(market_type: &str, sel_a: &str, sel_b: &str) -> bool {
    match market_type {
        "moneyline" => sel_a != sel_b && sel_a != "draw" && sel_b != "draw",
        "total" | "team_total" => {
            (sel_a == "over" && sel_b == "under") || (sel_a == "under" && sel_b == "over")
        }
        "spread" => sel_a != sel_b,
        _ => false,
    }
}
