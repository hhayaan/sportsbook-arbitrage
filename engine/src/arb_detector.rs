use crate::models::*;

/// Sports with 3-way moneyline (home/draw/away). 2-leg arbs are INVALID for these.
const THREE_WAY_SPORTS: &[&str] = &["SOCCER", "EPL", "LA_LIGA", "SERIE_A", "BUNDESLIGA", "LIGUE_1", "UCL", "MLS"];

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

        // Block 3-way moneyline arbs for soccer sports.
        let sport_upper = key.event_id.split(':').next().unwrap_or("");
        let is_three_way_sport = THREE_WAY_SPORTS.iter().any(|&s| s == sport_upper);
        if is_three_way_sport && key.market_type == "moneyline" {
            continue;
        }

        // Block partial-period moneyline arbs — ties are common
        // (1st inning MLB ML, 1st period NHL ML, etc.). Only full-game MLs are safe.
        if key.market_type == "moneyline" && key.period != "full_game" {
            continue;
        }

        // Per-venue vig self-consistency check: a real 2-way market should have
        // implied-probability sum (vig) of ~1.02-1.15. Outside 0.95-1.25 indicates
        // misclassification (3-way as 2-way, wrong period, doubleheader collision, etc.).
        let mut venue_vig_failed = false;
        {
            let mut by_venue: std::collections::HashMap<&str, Vec<&PriceEntry>> =
                std::collections::HashMap::new();
            for entry in entries.values() {
                by_venue.entry(entry.venue.as_str()).or_default().push(entry);
            }
            for (venue, venue_entries) in &by_venue {
                if venue_entries.len() < 2 { continue; }
                let mut best_vig: Option<f64> = None;
                for a in venue_entries {
                    for b in venue_entries {
                        if a.selection == b.selection { continue; }
                        if !are_selections_complementary(&key.market_type, &a.selection, &b.selection) {
                            continue;
                        }
                        let vig = 1.0 / a.decimal_odds + 1.0 / b.decimal_odds;
                        if best_vig.is_none() || (vig - 1.0).abs() < (best_vig.unwrap() - 1.0).abs() {
                            best_vig = Some(vig);
                        }
                    }
                }
                if let Some(vig) = best_vig {
                    if !vig.is_finite() || vig < 0.95 || vig > 1.25 {
                        tracing::warn!(
                            "[VIG CHECK] {} vig={:.3} on {} {} period={} event={} — skipping",
                            venue, vig, key.market_type, key.line_key, key.period, key.event_id
                        );
                        venue_vig_failed = true;
                        break;
                    }
                }
            }
        }
        if venue_vig_failed { continue; }

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

                        // Spread: entries must have OPPOSITE line signs.
                        // PIN PIT -1.5 + FD BAL +1.5 = complementary
                        // PIN PIT -1.5 + FD BAL -1.5 = both favorites, both can lose
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
                                        ea.venue, ea.selection, ea.line,
                                        eb.venue, eb.selection, eb.line,
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

                            // Warn on suspicious arbs but don't block
                            if arb_pct > 0.15 {
                                tracing::warn!(
                                    "[HIGH%] {:.1}% arb — possible mismatch: {} {} {}@{:.3} ({}) vs {}@{:.3} ({})",
                                    arb_pct * 100.0, key.event_id, key.market_type,
                                    ea.selection, ea.decimal_odds, ea.venue,
                                    eb.selection, eb.decimal_odds, eb.venue,
                                );
                            }

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

    candidates.sort_by(|a, b| b.arb_pct.partial_cmp(&a.arb_pct).unwrap_or(std::cmp::Ordering::Equal));
    candidates
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
