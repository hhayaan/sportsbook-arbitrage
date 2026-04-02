use crate::db::Db;
use crate::models::*;
use crate::stake_optimizer;
use futures::stream::{SplitSink, SplitStream};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::MaybeTlsStream;

pub type WsWriter = Arc<Mutex<SplitSink<tokio_tungstenite::WebSocketStream<MaybeTlsStream<TcpStream>>, Message>>>;
pub type WsReader = Arc<Mutex<SplitStream<tokio_tungstenite::WebSocketStream<MaybeTlsStream<TcpStream>>>>>;

const FILL_TIMEOUT_MS: u64 = 20000;

#[derive(Serialize)]
struct PlaceBetCommand {
    #[serde(rename = "type")]
    msg_type: String,
    attempt_id: String,
    venue: String,
    venue_market_id: String,
    selection: String,
    stake: f64,
    expected_odds: f64,
}

#[derive(Deserialize, Debug, Clone)]
pub struct FillReport {
    #[serde(rename = "type")]
    pub msg_type: Option<String>,
    pub attempt_id: String,
    pub status: String,
    pub accepted_stake: Option<f64>,
    pub accepted_odds: Option<f64>,
    pub error_message: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct BalanceUpdate {
    #[serde(rename = "type")]
    pub msg_type: Option<String>,
    pub venue: String,
    pub balance: f64,
}

#[derive(Deserialize, Debug)]
struct BalanceEntry {
    venue: String,
    balance: f64,
}

#[derive(Deserialize, Debug)]
struct BalancesResponse {
    #[serde(rename = "type")]
    msg_type: Option<String>,
    balances: Vec<BalanceEntry>,
}

#[derive(Deserialize, Debug)]
struct QuoteResult {
    #[serde(rename = "type")]
    msg_type: Option<String>,
    attempt_id: String,
    status: String,
    min_stake: Option<f64>,
    max_stake: Option<f64>,
    price: Option<f64>,
    error_message: Option<String>,
}

/// Connect to the bet placer WebSocket server.
pub async fn connect(url: &str) -> Result<(WsWriter, WsReader), String> {
    let (stream, _) = tokio_tungstenite::connect_async(url)
        .await
        .map_err(|e| format!("WS connect failed: {}", e))?;

    let (writer, reader) = stream.split();
    Ok((Arc::new(Mutex::new(writer)), Arc::new(Mutex::new(reader))))
}

/// Fetch real sportsbook balances from the bet placer.
pub async fn fetch_balances(
    writer: &WsWriter,
    reader: &WsReader,
) -> Result<Vec<(String, f64)>, String> {
    let payload = serde_json::json!({"type": "get_balances"}).to_string();

    {
        let mut w = writer.lock().await;
        w.send(Message::Text(payload))
            .await
            .map_err(|e| format!("WS send failed: {}", e))?;
    }

    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(10);
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Err("balance fetch timeout".to_string());
        }

        let msg = {
            let mut r = reader.lock().await;
            match tokio::time::timeout(remaining, r.next()).await {
                Ok(Some(Ok(Message::Text(text)))) => Some(text),
                Ok(Some(Ok(_))) => continue,
                Ok(Some(Err(e))) => return Err(format!("WS read error: {}", e)),
                Ok(None) => return Err("WS connection closed".to_string()),
                Err(_) => return Err("balance fetch timeout".to_string()),
            }
        };

        if let Some(text) = msg {
            if let Ok(resp) = serde_json::from_str::<BalancesResponse>(&text) {
                if resp.msg_type.as_deref() == Some("balances") {
                    return Ok(resp.balances.into_iter().map(|b| (b.venue, b.balance)).collect());
                }
            }
        }
    }
}

/// Pre-flight quote for Pinnacle — returns min/max stake and current price.
pub async fn quote_leg(
    writer: &WsWriter,
    reader: &WsReader,
    attempt_id: &str,
    venue: &str,
    venue_market_id: &str,
) -> Result<(f64, f64, f64), String> {
    let payload = serde_json::json!({
        "type": "quote_bet",
        "attempt_id": attempt_id,
        "venue": venue,
        "venue_market_id": venue_market_id,
    }).to_string();

    {
        let mut w = writer.lock().await;
        w.send(Message::Text(payload))
            .await
            .map_err(|e| format!("WS send failed: {}", e))?;
    }

    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(10);
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Err("quote timeout".to_string());
        }
        let msg = {
            let mut r = reader.lock().await;
            match tokio::time::timeout(remaining, r.next()).await {
                Ok(Some(Ok(Message::Text(text)))) => Some(text),
                Ok(Some(Ok(_))) => continue,
                Ok(Some(Err(e))) => return Err(format!("WS read error: {}", e)),
                Ok(None) => return Err("WS connection closed".to_string()),
                Err(_) => return Err("quote timeout".to_string()),
            }
        };
        if let Some(text) = msg {
            if let Ok(q) = serde_json::from_str::<QuoteResult>(&text) {
                if q.msg_type.as_deref() == Some("quote_result") && q.attempt_id == attempt_id {
                    if q.status != "ok" {
                        return Err(format!("quote failed: {}", q.error_message.unwrap_or_default()));
                    }
                    return Ok((
                        q.min_stake.unwrap_or(0.0),
                        q.max_stake.unwrap_or(999999.0),
                        q.price.unwrap_or(0.0),
                    ));
                }
            }
        }
    }
}

/// Drain any pending balance_update messages from the WS and update the DB.
/// Non-blocking — reads for up to 500ms then returns.
async fn drain_balance_updates(reader: &WsReader, db: &Arc<Db>) {
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_millis(500);
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        let msg = {
            let mut r = reader.lock().await;
            match tokio::time::timeout(remaining, r.next()).await {
                Ok(Some(Ok(Message::Text(text)))) => Some(text),
                _ => break,
            }
        };
        if let Some(text) = msg {
            if let Ok(update) = serde_json::from_str::<BalanceUpdate>(&text) {
                if update.msg_type.as_deref() == Some("balance_update") {
                    tracing::info!(
                        "[BALANCE] {} real balance: ${:.2}",
                        update.venue, update.balance
                    );
                    db.update_balance(&update.venue, update.balance);
                }
            }
        }
    }
}

/// Place a single leg via WebSocket command to the bet placer.
/// Sends PlaceBetCommand, waits for FillReport with matching attempt_id.
async fn place_leg(
    writer: &WsWriter,
    reader: &WsReader,
    attempt_id: &str,
    venue: &str,
    venue_market_id: &str,
    selection: &str,
    stake: f64,
    expected_odds: f64,
) -> Result<FillReport, String> {
    let cmd = PlaceBetCommand {
        msg_type: "place_bet".to_string(),
        attempt_id: attempt_id.to_string(),
        venue: venue.to_string(),
        venue_market_id: venue_market_id.to_string(),
        selection: selection.to_string(),
        stake,
        expected_odds,
    };

    let payload = serde_json::to_string(&cmd).map_err(|e| format!("serialize: {}", e))?;

    tracing::info!(
        venue = %venue,
        stake = stake,
        selection = %selection,
        "Sending PLACE_BET via WS"
    );

    // Send command
    {
        let mut w = writer.lock().await;
        w.send(Message::Text(payload))
            .await
            .map_err(|e| format!("WS send failed: {}", e))?;
    }

    // Wait for fill report with matching attempt_id
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_millis(FILL_TIMEOUT_MS);

    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Err(format!("fill timeout after {}ms", FILL_TIMEOUT_MS));
        }

        let msg = {
            let mut r = reader.lock().await;
            match tokio::time::timeout(remaining, r.next()).await {
                Ok(Some(Ok(Message::Text(text)))) => Some(text),
                Ok(Some(Ok(_))) => continue,
                Ok(Some(Err(e))) => return Err(format!("WS read error: {}", e)),
                Ok(None) => return Err("WS connection closed".to_string()),
                Err(_) => return Err(format!("fill timeout after {}ms", FILL_TIMEOUT_MS)),
            }
        };

        if let Some(text) = msg {
            if let Ok(fill) = serde_json::from_str::<FillReport>(&text) {
                if fill.msg_type.as_deref() == Some("fill_report") && fill.attempt_id == attempt_id {
                    tracing::info!(
                        venue = %venue,
                        status = %fill.status,
                        "Fill received via WS"
                    );
                    return Ok(fill);
                }
            }
        }
    }
}

/// Venues we can actually place bets on via Patchright.
const BETTABLE_VENUES: &[&str] = &["fanduel", "draftkings", "pinnacle"];

/// Live execution: size the trade, place leg1 (soft book first), then leg2.
/// If leg1 fails, leg2 is never placed. Returns the trade record on success.
pub async fn try_execute(
    writer: &WsWriter,
    reader: &WsReader,
    db: &Arc<Db>,
    candidate: &ArbCandidate,
    settings: &Settings,
) -> Option<PaperTrade> {
    if !BETTABLE_VENUES.contains(&candidate.leg1_venue.as_str())
        || !BETTABLE_VENUES.contains(&candidate.leg2_venue.as_str())
    {
        tracing::debug!(
            "Skipping live trade: {} and/or {} not bettable",
            candidate.leg1_venue, candidate.leg2_venue
        );
        return None;
    }

    let min_arb_pct = settings.min_arb_pct / 100.0;
    if candidate.arb_pct < min_arb_pct {
        return None;
    }

    let leg1_balance = db.get_balance(&candidate.leg1_venue)?;
    let leg2_balance = db.get_balance(&candidate.leg2_venue)?;

    let capital = match settings.stake_mode.as_str() {
        "proportional" => {
            let smaller = leg1_balance.min(leg2_balance);
            (smaller * settings.proportional_pct / 100.0).min(settings.max_stake * 2.0)
        }
        _ => settings.fixed_stake * 2.0,
    };

    if capital <= 0.0 {
        return None;
    }

    let plan = stake_optimizer::compute_stakes(candidate, capital)?;

    if plan.leg1_stake > leg1_balance || plan.leg2_stake > leg2_balance {
        tracing::warn!(
            "Insufficient balance: need ${:.2}+${:.2}, have ${:.2}+${:.2}",
            plan.leg1_stake, plan.leg2_stake, leg1_balance, leg2_balance
        );
        return None;
    }

    if plan.leg1_stake > settings.max_stake || plan.leg2_stake > settings.max_stake {
        return None;
    }

    let attempt_id = uuid::Uuid::new_v4().to_string();

    // Determine leg order: Pinnacle first (needs quote), then FD (soft book), then DK
    let leg1_is_first = if candidate.leg1_venue == "pinnacle" {
        true
    } else if candidate.leg2_venue == "pinnacle" {
        false
    } else if candidate.leg1_venue == "fanduel" {
        true
    } else {
        false
    };

    let (first_venue, first_sel, mut first_stake, first_odds, first_mkt_id,
         second_venue, second_sel, mut second_stake, second_odds, second_mkt_id) =
        if leg1_is_first {
            (&candidate.leg1_venue, &candidate.leg1_selection, plan.leg1_stake, candidate.leg1_odds, candidate.leg1_venue_market_id.clone(),
             &candidate.leg2_venue, &candidate.leg2_selection, plan.leg2_stake, candidate.leg2_odds, candidate.leg2_venue_market_id.clone())
        } else {
            (&candidate.leg2_venue, &candidate.leg2_selection, plan.leg2_stake, candidate.leg2_odds, candidate.leg2_venue_market_id.clone(),
             &candidate.leg1_venue, &candidate.leg1_selection, plan.leg1_stake, candidate.leg1_odds, candidate.leg1_venue_market_id.clone())
        };

    // ── Pre-flight quote for Pinnacle leg ──
    let has_pinnacle = first_venue == "pinnacle" || second_venue == "pinnacle";
    if has_pinnacle {
        let (pin_venue, pin_stake, pin_odds, pin_mkt_id, other_stake, other_odds) =
            if first_venue == "pinnacle" {
                (first_venue.as_str(), first_stake, first_odds, &first_mkt_id, second_stake, second_odds)
            } else {
                (second_venue.as_str(), second_stake, second_odds, &second_mkt_id, first_stake, first_odds)
            };

        match quote_leg(writer, reader, &format!("{}-Q", attempt_id), pin_venue, pin_mkt_id).await {
            Ok((min_stake, max_stake, _quoted_price)) => {
                let mut adj_pin = pin_stake;
                let mut adj_other = other_stake;

                // Bump up to minimum if needed
                if adj_pin < min_stake {
                    tracing::info!("[QUOTE] PIN stake ${:.2} below min ${:.2}, adjusting", adj_pin, min_stake);
                    adj_pin = min_stake;
                    // Recalculate other leg to maintain arb ratio
                    adj_other = adj_pin * (1.0 / other_odds) / (1.0 / pin_odds);
                    adj_other = (adj_other * 100.0).round() / 100.0;
                }

                // Cap at maximum if needed
                if adj_pin > max_stake {
                    tracing::info!("[QUOTE] PIN stake ${:.2} above max ${:.2}, adjusting", adj_pin, max_stake);
                    adj_pin = max_stake;
                    adj_other = adj_pin * (1.0 / other_odds) / (1.0 / pin_odds);
                    adj_other = (adj_other * 100.0).round() / 100.0;
                }

                // Verify total still within user's max
                if adj_pin > settings.max_stake || adj_other > settings.max_stake {
                    tracing::info!("[QUOTE] Adjusted stakes exceed max_stake, skipping");
                    return None;
                }

                // Apply adjusted stakes
                if first_venue == "pinnacle" {
                    first_stake = (adj_pin * 100.0).round() / 100.0;
                    second_stake = adj_other;
                } else {
                    second_stake = (adj_pin * 100.0).round() / 100.0;
                    first_stake = adj_other;
                }

                tracing::info!("[QUOTE] Adjusted stakes: {} ${:.2}, {} ${:.2}",
                    first_venue, first_stake, second_venue, second_stake);
            }
            Err(e) => {
                tracing::error!("[QUOTE] Failed: {}. Skipping arb.", e);
                return None;
            }
        }
    }

    // ── LEG 1 ──
    tracing::info!(
        "LEG 1: {} ${:.2} on {} @ {:.3}",
        first_venue, first_stake, first_sel, first_odds
    );

    let fill1 = match place_leg(
        writer, reader, &format!("{}-L1", attempt_id),
        first_venue, &first_mkt_id, first_sel, first_stake, first_odds,
    ).await {
        Ok(f) if f.status == "filled" => f,
        Ok(f) => {
            tracing::error!(
                "LEG 1 FAILED: {} — {}.",
                f.status, f.error_message.unwrap_or_default()
            );
            return None;
        }
        Err(e) => {
            tracing::error!("LEG 1 ERROR: {}.", e);
            return None;
        }
    };

    // Drain balance update after leg 1 fill
    drain_balance_updates(reader, db).await;

    let s1 = fill1.accepted_stake.unwrap_or(first_stake);
    let o1 = fill1.accepted_odds.unwrap_or(first_odds);

    // ── LEG 2 ──
    tracing::info!(
        "LEG 2: {} ${:.2} on {} @ {:.3}",
        second_venue, second_stake, second_sel, second_odds
    );

    let fill2 = match place_leg(
        writer, reader, &format!("{}-L2", attempt_id),
        second_venue, &second_mkt_id, second_sel, second_stake, second_odds,
    ).await {
        Ok(f) if f.status == "filled" => f,
        Ok(f) => {
            tracing::error!(
                "LEG 2 FAILED: {} — {}. WARNING: LEG 1 PLACED, EXPOSED!",
                f.status, f.error_message.unwrap_or_default()
            );
            drain_balance_updates(reader, db).await;
            return None;
        }
        Err(e) => {
            tracing::error!("LEG 2 ERROR: {}. WARNING: LEG 1 PLACED, EXPOSED!", e);
            return None;
        }
    };

    // Drain balance update after leg 2 fill
    drain_balance_updates(reader, db).await;

    let s2 = fill2.accepted_stake.unwrap_or(second_stake);
    let o2 = fill2.accepted_odds.unwrap_or(second_odds);

    // Both legs filled — calculate theoretical profit for the trade record
    let payout1 = s1 * o1;
    let payout2 = s2 * o2;
    let cost = s1 + s2;
    let profit = payout1.min(payout2) - cost;

    let now = chrono::Utc::now().to_rfc3339();
    let trade = PaperTrade {
        id: attempt_id,
        detected_at: now.clone(),
        executed_at: now,
        event_id: candidate.event_id.clone(),
        sport: candidate.sport.clone(),
        market_type: candidate.market_type.clone(),
        leg1_venue: candidate.leg1_venue.clone(),
        leg1_selection: candidate.leg1_selection.clone(),
        leg1_odds: o1,
        leg1_stake: s1,
        leg2_venue: candidate.leg2_venue.clone(),
        leg2_selection: candidate.leg2_selection.clone(),
        leg2_odds: o2,
        leg2_stake: s2,
        sigma: candidate.sigma,
        theoretical_profit: profit,
        status: "executed_live".to_string(),
    };

    db.insert_trade(&trade);

    tracing::info!(
        "BOTH LEGS FILLED! profit=${:.2} | {}@{:.3} ${:.2} + {}@{:.3} ${:.2}",
        profit, first_sel, o1, s1, second_sel, o2, s2
    );

    Some(trade)
}
