use crate::models::{Book, MarketIdentityKey};

#[derive(Debug, Clone)]
pub struct RescuePlan {
    pub venue: String,
    pub selection: String,
    pub odds: f64,
    pub stake: f64,
    pub worst_case_loss: f64,
}

/// Compute a rescue hedge when leg 2 fails after leg 1 fills.
///
/// Finds best available odds for the complementary selection on ANY venue,
/// then calculates the rescue stake to minimize worst-case loss.
///
/// rescue_stake = leg1_stake * leg1_odds / rescue_odds
pub fn compute_rescue(
    book: &Book,
    identity_key: &MarketIdentityKey,
    leg2_selection: &str,
    leg1_accepted_stake: f64,
    leg1_accepted_odds: f64,
    max_rescue_loss: f64,
) -> Option<RescuePlan> {
    let entries = book.get_entries(identity_key)?;

    // Find best odds for leg2_selection on any venue
    let mut best_odds: f64 = 0.0;
    let mut best_venue = String::new();

    for (vs_key, entry) in entries.iter() {
        if vs_key.selection == leg2_selection && entry.decimal_odds > best_odds {
            best_odds = entry.decimal_odds;
            best_venue = entry.venue.clone();
        }
    }

    if best_odds <= 1.0 {
        return None;
    }

    let leg1_payout = leg1_accepted_stake * leg1_accepted_odds;
    let rescue_stake = leg1_payout / best_odds;
    let rescue_stake = (rescue_stake * 100.0).round() / 100.0;

    let total_stake = leg1_accepted_stake + rescue_stake;
    let leg1_outcome_pnl = leg1_payout - total_stake;
    let leg2_outcome_pnl = rescue_stake * best_odds - total_stake;
    let guaranteed_pnl = leg1_outcome_pnl.min(leg2_outcome_pnl);
    let worst_case_loss = (-guaranteed_pnl).max(0.0);

    if worst_case_loss > max_rescue_loss {
        return None;
    }

    if rescue_stake < 1.0 {
        return None;
    }

    Some(RescuePlan {
        venue: best_venue,
        selection: leg2_selection.to_string(),
        odds: best_odds,
        stake: rescue_stake,
        worst_case_loss,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Book, CanonicalMarket};

    fn make_market(venue: &str, selection: &str, odds: f64) -> CanonicalMarket {
        CanonicalMarket {
            event_id: "NBA:2026-03-15:A-vs-B".into(),
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
            american_odds: Some(0.0),
            venue: venue.into(),
            venue_market_id: format!("{}-ml", venue),
            venue_event_id: format!("{}-ev", venue),
            received_at_ms: 1000,
        }
    }

    #[test]
    fn test_rescue_basic() {
        let mut book = Book::new(60000);
        book.update(&make_market("fanduel", "B", 2.5));
        book.update(&make_market("bovada", "B", 2.3));
        book.update(&make_market("draftkings", "A", 1.8));

        let identity = MarketIdentityKey::from_market(&make_market("draftkings", "A", 1.8));

        let plan = compute_rescue(&book, &identity, "B", 100.0, 1.8, 50.0);
        assert!(plan.is_some());
        let p = plan.unwrap();
        assert_eq!(p.venue, "fanduel");
        assert!((p.odds - 2.5).abs() < 0.001);
        assert!((p.stake - 72.00).abs() < 0.01);
        assert!((p.worst_case_loss - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_rescue_too_expensive() {
        let mut book = Book::new(60000);
        book.update(&make_market("fanduel", "B", 1.1));

        let identity = MarketIdentityKey::from_market(&make_market("draftkings", "A", 3.0));
        let plan = compute_rescue(&book, &identity, "B", 200.0, 3.0, 25.0);
        assert!(plan.is_none());
    }

    #[test]
    fn test_rescue_no_odds_available() {
        let book = Book::new(60000);
        let identity = MarketIdentityKey::from_market(&make_market("draftkings", "A", 1.8));
        let plan = compute_rescue(&book, &identity, "B", 100.0, 1.8, 50.0);
        assert!(plan.is_none());
    }
}
