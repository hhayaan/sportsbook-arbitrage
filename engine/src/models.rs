use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── Canonical market (normalized from any venue) ────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalMarket {
    pub event_id: String,
    pub sport: String,
    pub league: String,
    pub start_time_utc: String,
    pub market_type: String, // moneyline | spread | total | team_total | player_prop | alt_spread | alt_total
    pub selection: String,
    pub line: Option<f64>,
    pub period: String,
    pub is_live: bool,
    pub player_name: Option<String>,
    pub prop_type: Option<String>,
    pub decimal_odds: f64,
    pub american_odds: Option<f64>,
    pub venue: String,
    pub venue_market_id: String,
    pub venue_event_id: String,
    pub received_at_ms: u64,
}

// ── In-memory order book ────────────────────────────────────────────────────

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct MarketIdentityKey {
    pub event_id: String,
    pub market_type: String,
    pub line_key: String,
    pub period: String,
    pub is_live: bool,
    pub player_key: String,
}

impl MarketIdentityKey {
    pub fn from_market(m: &CanonicalMarket) -> Self {
        let line_key = match m.line {
            Some(l) => {
                // For spreads, use absolute value so both sides (+1.5 and -1.5)
                // group together. Otherwise same-side bets from different venues
                // get paired as if they're opposite sides.
                if m.market_type == "spread" || m.market_type == "alt_spread" {
                    format!("{:.1}", l.abs())
                } else {
                    format!("{:.1}", l)
                }
            }
            None => "null".to_string(),
        };
        let player_key = match (&m.player_name, &m.prop_type) {
            (Some(name), Some(prop)) => format!("{}:{}", name, prop),
            _ => String::new(),
        };
        MarketIdentityKey {
            event_id: m.event_id.clone(),
            market_type: m.market_type.clone(),
            line_key,
            period: m.period.clone(),
            // Always false: DK hardcodes is_live=false, PIN uses true for same live game.
            // Using actual value breaks cross-venue pairing on live games.
            is_live: false,
            player_key,
        }
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct VenueSelectionKey {
    pub venue: String,
    pub selection: String,
}

#[derive(Debug, Clone)]
pub struct PriceEntry {
    pub venue: String,
    pub selection: String,
    pub decimal_odds: f64,
    pub received_at_ms: u64,
    pub sport: String,
    pub event_id: String,
    pub market_type: String,
    pub line: Option<f64>,
    pub is_live: bool,
    pub start_time_utc: String,
    pub venue_market_id: String,
}

impl PriceEntry {
    pub fn from_market(m: &CanonicalMarket) -> Self {
        PriceEntry {
            venue: m.venue.clone(),
            selection: m.selection.clone(),
            decimal_odds: m.decimal_odds,
            received_at_ms: m.received_at_ms,
            sport: m.sport.clone(),
            event_id: m.event_id.clone(),
            market_type: m.market_type.clone(),
            line: m.line,
            is_live: m.is_live,
            start_time_utc: m.start_time_utc.clone(),
            venue_market_id: m.venue_market_id.clone(),
        }
    }
}

pub struct Book {
    entries: HashMap<MarketIdentityKey, HashMap<VenueSelectionKey, PriceEntry>>,
    max_age_ms: u64,
}

impl Book {
    pub fn new(max_age_ms: u64) -> Self {
        Book {
            entries: HashMap::new(),
            max_age_ms,
        }
    }

    pub fn update(&mut self, market: &CanonicalMarket) {
        let identity = MarketIdentityKey::from_market(market);
        let vs_key = VenueSelectionKey {
            venue: market.venue.clone(),
            selection: market.selection.clone(),
        };
        let entry = PriceEntry::from_market(market);
        self.entries
            .entry(identity)
            .or_default()
            .insert(vs_key, entry);
    }

    /// Refresh timestamp on an existing entry without changing odds.
    pub fn touch(&mut self, market: &CanonicalMarket) {
        let identity = MarketIdentityKey::from_market(market);
        let vs_key = VenueSelectionKey {
            venue: market.venue.clone(),
            selection: market.selection.clone(),
        };
        if let Some(inner) = self.entries.get_mut(&identity) {
            if let Some(entry) = inner.get_mut(&vs_key) {
                entry.received_at_ms = market.received_at_ms;
            }
        }
    }

    pub fn expire(&mut self, now_ms: u64) {
        for inner in self.entries.values_mut() {
            inner.retain(|_, entry| {
                now_ms.saturating_sub(entry.received_at_ms) <= self.max_age_ms
            });
        }
        self.entries.retain(|_, inner| !inner.is_empty());
    }

    pub fn get_entries(
        &self,
        key: &MarketIdentityKey,
    ) -> Option<&HashMap<VenueSelectionKey, PriceEntry>> {
        self.entries.get(key)
    }

    pub fn identity_keys(&self) -> Vec<MarketIdentityKey> {
        self.entries.keys().cloned().collect()
    }

    pub fn total_entries(&self) -> usize {
        self.entries.values().map(|m| m.len()).sum()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

// ── Arb candidate ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArbCandidate {
    pub event_id: String,
    pub sport: String,
    pub market_type: String,
    pub period: String,
    pub start_time_utc: String,
    pub leg1_venue: String,
    pub leg1_selection: String,
    pub leg1_odds: f64,
    pub leg1_received_at_ms: u64,
    pub leg1_venue_market_id: String,
    pub leg2_venue: String,
    pub leg2_selection: String,
    pub leg2_odds: f64,
    pub leg2_received_at_ms: u64,
    pub leg2_venue_market_id: String,
    pub sigma: f64,
    pub arb_pct: f64,
    pub roi: f64,
    pub line: Option<f64>,
    pub is_live: bool,
}

// ── Stake plan ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StakePlan {
    pub leg1_stake: f64,
    pub leg2_stake: f64,
    pub total_deployed: f64,
    pub guaranteed_payout: f64,
    pub guaranteed_profit: f64,
}

// ── Paper trade ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaperTrade {
    pub id: String,
    pub detected_at: String,
    pub executed_at: String,
    pub event_id: String,
    pub sport: String,
    pub market_type: String,
    pub leg1_venue: String,
    pub leg1_selection: String,
    pub leg1_odds: f64,
    pub leg1_stake: f64,
    pub leg2_venue: String,
    pub leg2_selection: String,
    pub leg2_odds: f64,
    pub leg2_stake: f64,
    pub sigma: f64,
    pub theoretical_profit: f64,
    pub status: String,
}

#[derive(Debug, Clone)]
pub struct ExecutionResult {
    pub trade: Option<PaperTrade>,
    pub family_lock_reason: Option<String>,
}

impl ExecutionResult {
    pub fn trade(trade: PaperTrade) -> Self {
        Self {
            trade: Some(trade),
            family_lock_reason: None,
        }
    }

    pub fn family_lock(reason: impl Into<String>) -> Self {
        Self {
            trade: None,
            family_lock_reason: Some(reason.into()),
        }
    }
}

// ── Arb opportunity (logged) ────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArbOpportunity {
    pub id: String,
    pub detected_at: String,
    pub event_id: String,
    pub sport: String,
    pub market_type: String,
    pub sigma: f64,
    pub arb_pct: f64,
    pub leg1_venue: String,
    pub leg1_selection: String,
    pub leg1_odds: f64,
    pub leg2_venue: String,
    pub leg2_selection: String,
    pub leg2_odds: f64,
    pub traded: bool,
}

// ── Book balance ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookBalance {
    pub venue: String,
    pub balance: f64,
    pub initial_balance: f64,
    pub updated_at: String,
}

// ── Settings ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub enabled: bool,
    pub min_arb_pct: f64,
    pub max_stake: f64,
    pub stake_mode: String,       // "fixed" | "proportional"
    pub fixed_stake: f64,
    pub proportional_pct: f64,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            enabled: true,
            min_arb_pct: 0.5,
            max_stake: 50.0,
            stake_mode: "fixed".to_string(),
            fixed_stake: 25.0,
            proportional_pct: 5.0,
        }
    }
}

// ── Stats ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stats {
    pub total_pnl: f64,
    pub trade_count: i64,
    pub avg_arb_pct: f64,
    pub best_trade_profit: f64,
    pub total_arbs_detected: i64,
    pub total_deployed: f64,
}

// ── WebSocket events ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WsEvent {
    #[serde(rename = "arb_detected")]
    ArbDetected { arb: ArbOpportunity },
    #[serde(rename = "trade_executed")]
    TradeExecuted { trade: PaperTrade },
    #[serde(rename = "balance_updated")]
    BalanceUpdated { balances: Vec<BookBalance> },
    #[serde(rename = "settings_updated")]
    SettingsUpdated { settings: Settings },
    #[serde(rename = "poll_status")]
    PollStatus { venue: String, markets: usize },
    #[serde(rename = "error")]
    Error { message: String },
}
