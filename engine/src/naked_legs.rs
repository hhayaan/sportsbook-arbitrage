use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NakedLeg {
    pub id: String,
    pub attempt_id: String,
    pub created_at: String,
    pub updated_at: String,
    pub venue: String,
    pub venue_market_id: String,
    pub selection: String,
    pub stake: f64,
    pub odds: f64,
    pub purchase_id: Option<String>,
    pub bet_id: Option<String>,
    pub event_id: String,
    pub sport: String,
    pub market_type: String,
    pub pin_venue_market_id: Option<String>,
    pub pin_selection: Option<String>,
    pub status: String,
    pub cashout_attempts: u32,
    pub last_checked_at: Option<String>,
    pub notes: Option<String>,
}

fn ledger_path() -> PathBuf {
    std::env::var("NAKED_LEGS_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("data").join("naked-legs.json"))
}

pub fn load_naked_legs() -> Vec<NakedLeg> {
    let path = ledger_path();
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    serde_json::from_str(&text).unwrap_or_else(|e| {
        tracing::warn!("Could not parse naked-leg ledger: {}", e);
        Vec::new()
    })
}

pub fn save_naked_legs(legs: &[NakedLeg]) -> io::Result<()> {
    let path = ledger_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(legs)?;
    fs::write(path, text)
}

pub fn add_naked_leg(leg: NakedLeg) -> io::Result<()> {
    let mut legs = load_naked_legs();
    legs.retain(|existing| existing.attempt_id != leg.attempt_id);
    legs.push(leg);
    save_naked_legs(&legs)
}

pub fn remove_naked_leg(id_or_attempt: &str) -> io::Result<()> {
    let mut legs = load_naked_legs();
    let before = legs.len();
    legs.retain(|leg| leg.id != id_or_attempt && leg.attempt_id != id_or_attempt);
    if legs.len() != before {
        save_naked_legs(&legs)?;
    }
    Ok(())
}

pub fn new_naked_leg(
    attempt_id: &str,
    venue: &str,
    venue_market_id: &str,
    selection: &str,
    stake: f64,
    odds: f64,
    purchase_id: Option<String>,
    bet_id: Option<String>,
    event_id: &str,
    sport: &str,
    market_type: &str,
    pin_venue_market_id: Option<String>,
    pin_selection: Option<String>,
    notes: Option<String>,
) -> NakedLeg {
    let now = chrono::Utc::now().to_rfc3339();
    NakedLeg {
        id: uuid::Uuid::new_v4().to_string(),
        attempt_id: attempt_id.to_string(),
        created_at: now.clone(),
        updated_at: now,
        venue: venue.to_string(),
        venue_market_id: venue_market_id.to_string(),
        selection: selection.to_string(),
        stake,
        odds,
        purchase_id,
        bet_id,
        event_id: event_id.to_string(),
        sport: sport.to_string(),
        market_type: market_type.to_string(),
        pin_venue_market_id,
        pin_selection,
        status: "open".to_string(),
        cashout_attempts: 0,
        last_checked_at: None,
        notes,
    }
}
