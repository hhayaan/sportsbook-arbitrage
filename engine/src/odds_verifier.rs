use crate::models::{ArbCandidate, Book, MarketIdentityKey, VenueSelectionKey};

/// Verify both legs' odds haven't drifted beyond threshold since detection.
pub fn verify_both_legs(
    book: &Book,
    candidate: &ArbCandidate,
    max_drift: f64,
) -> Result<(), String> {
    verify_leg(book, candidate, 1, max_drift)?;
    verify_leg(book, candidate, 2, max_drift)?;
    Ok(())
}

/// Verify a single leg's current book odds against detection-time odds.
pub fn verify_leg(
    book: &Book,
    candidate: &ArbCandidate,
    leg_num: u8,
    max_drift: f64,
) -> Result<f64, String> {
    let (venue, selection, expected_odds, leg_label) = match leg_num {
        1 => (
            &candidate.leg1_venue,
            &candidate.leg1_selection,
            candidate.leg1_odds,
            "leg1",
        ),
        2 => (
            &candidate.leg2_venue,
            &candidate.leg2_selection,
            candidate.leg2_odds,
            "leg2",
        ),
        _ => unreachable!(),
    };

    let line_key = match candidate.line {
        Some(l) => {
            if candidate.market_type == "spread" || candidate.market_type == "alt_spread" {
                format!("{:.1}", l.abs())
            } else {
                format!("{:.1}", l)
            }
        }
        None => "null".to_string(),
    };

    let identity_key = MarketIdentityKey {
        event_id: candidate.event_id.clone(),
        market_type: candidate.market_type.clone(),
        line_key,
        period: candidate.period.clone(),
        is_live: false,
        player_key: String::new(),
    };

    let entries = book
        .get_entries(&identity_key)
        .ok_or_else(|| format!("{}: market no longer in book", leg_label))?;

    let vs_key = VenueSelectionKey {
        venue: venue.clone(),
        selection: selection.clone(),
    };

    let entry = entries
        .get(&vs_key)
        .ok_or_else(|| format!("{}: {}/{} no longer in book", leg_label, venue, selection))?;

    let current_odds = entry.decimal_odds;
    let drift_pct = (current_odds - expected_odds).abs() / expected_odds;

    if drift_pct > max_drift {
        return Err(format!(
            "{}: odds drifted {:.1}% (expected={:.3}, current={:.3}, max={:.1}%)",
            leg_label,
            drift_pct * 100.0,
            expected_odds,
            current_odds,
            max_drift * 100.0
        ));
    }

    Ok(current_odds)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Book, CanonicalMarket};

    fn make_market(venue: &str, selection: &str, odds: f64) -> CanonicalMarket {
        CanonicalMarket {
            event_id: "NBA:2026-03-15:LAL-vs-BOS".into(),
            sport: "NBA".into(),
            league: "NBA".into(),
            start_time_utc: "2026-03-15T20:00:00Z".into(),
            market_type: "moneyline".into(),
            selection: selection.into(),
            line: None,
            period: "full_game".into(),
            is_live: false,
            player_name: None,
            prop_type: None,
            decimal_odds: odds,
            american_odds: Some(100.0),
            venue: venue.into(),
            venue_market_id: format!("{}-ml", venue),
            venue_event_id: format!("{}-ev", venue),
            received_at_ms: 1000,
        }
    }

    fn make_candidate(odds1: f64, odds2: f64) -> ArbCandidate {
        let sigma = 1.0 / odds1 + 1.0 / odds2;
        ArbCandidate {
            event_id: "NBA:2026-03-15:LAL-vs-BOS".into(),
            sport: "NBA".into(),
            market_type: "moneyline".into(),
            period: "full_game".into(),
            start_time_utc: "2026-03-15T20:00:00Z".into(),
            leg1_venue: "draftkings".into(),
            leg1_selection: "LAL".into(),
            leg1_odds: odds1,
            leg1_received_at_ms: 1000,
            leg1_venue_market_id: "dk-ml".into(),
            leg2_venue: "fanduel".into(),
            leg2_selection: "BOS".into(),
            leg2_odds: odds2,
            leg2_received_at_ms: 1000,
            leg2_venue_market_id: "fd-ml".into(),
            sigma,
            arb_pct: 1.0 - sigma,
            roi: 1.0 / sigma - 1.0,
            line: None,
            is_live: false,
        }
    }

    #[test]
    fn test_verify_passes_within_threshold() {
        let mut book = Book::new(60000);
        book.update(&make_market("draftkings", "LAL", 2.50));
        book.update(&make_market("fanduel", "BOS", 1.714));
        let candidate = make_candidate(2.50, 1.714);
        assert!(verify_both_legs(&book, &candidate, 0.03).is_ok());
    }

    #[test]
    fn test_verify_passes_with_small_drift() {
        let mut book = Book::new(60000);
        book.update(&make_market("draftkings", "LAL", 2.525)); // +1%
        book.update(&make_market("fanduel", "BOS", 1.714));
        let candidate = make_candidate(2.50, 1.714);
        assert!(verify_both_legs(&book, &candidate, 0.03).is_ok());
    }

    #[test]
    fn test_verify_fails_beyond_threshold() {
        let mut book = Book::new(60000);
        book.update(&make_market("draftkings", "LAL", 2.375)); // -5%
        book.update(&make_market("fanduel", "BOS", 1.714));
        let candidate = make_candidate(2.50, 1.714);
        assert!(verify_both_legs(&book, &candidate, 0.03).is_err());
    }

    #[test]
    fn test_verify_missing_entry_fails() {
        let book = Book::new(60000);
        let candidate = make_candidate(2.50, 1.714);
        assert!(verify_both_legs(&book, &candidate, 0.03).is_err());
    }

    #[test]
    fn test_verify_catches_leg2_drift() {
        let mut book = Book::new(60000);
        book.update(&make_market("draftkings", "LAL", 2.50));
        book.update(&make_market("fanduel", "BOS", 1.60)); // ~6.6% drift
        let candidate = make_candidate(2.50, 1.714);
        assert!(verify_both_legs(&book, &candidate, 0.03).is_err());
    }
}
