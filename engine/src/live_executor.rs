use crate::attempt_telemetry;
use crate::db::Db;
use crate::execution_math::{compute_pin_hedge_from_soft_fill, AuthenticatedQuote};
use crate::models::*;
use crate::naked_legs;
use crate::pin_yank;
use crate::stake_optimizer;
use crate::status_classifier::{classify_cashout_confirmation, CashoutConfirmation};
use futures::stream::{SplitSink, SplitStream};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::MaybeTlsStream;

pub type WsWriter =
    Arc<Mutex<SplitSink<tokio_tungstenite::WebSocketStream<MaybeTlsStream<TcpStream>>, Message>>>;
pub type WsReader =
    Arc<Mutex<SplitStream<tokio_tungstenite::WebSocketStream<MaybeTlsStream<TcpStream>>>>>;

const DEFAULT_FILL_TIMEOUT_MS: u64 = 20000;
const DEFAULT_PIN_FILL_TIMEOUT_MS: u64 = 45000;

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
    pub purchase_id: Option<String>,
    pub bet_id: Option<String>,
    pub raw_status: Option<String>,
    pub reconciliation_hint: Option<String>,
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
    error: Option<String>,
}

#[derive(Deserialize, Debug)]
struct ValidateSoftResult {
    #[serde(rename = "type")]
    msg_type: Option<String>,
    attempt_id: String,
    status: String,
    price: Option<f64>,
    display_odds: Option<Value>,
    error_message: Option<String>,
    error_code: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
struct BetStatusResult {
    #[serde(rename = "type")]
    msg_type: Option<String>,
    attempt_id: Option<String>,
    venue: Option<String>,
    status: Option<String>,
    purchase_id: Option<String>,
    bet_id: Option<String>,
    raw_status: Option<String>,
    settled_at: Option<String>,
    outcome: Option<String>,
    cashout_available: Option<bool>,
    cashout_offer: Option<f64>,
    error: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
struct CashoutResult {
    #[serde(rename = "type")]
    msg_type: Option<String>,
    attempt_id: Option<String>,
    success: Option<bool>,
    confirmed: Option<bool>,
    unconfirmed: Option<bool>,
    submitted: Option<bool>,
    terminal_non_cashout: Option<bool>,
    payout: Option<f64>,
    payout_claimed: Option<f64>,
    raw_status: Option<String>,
    reason: Option<String>,
    error: Option<String>,
}

enum PinPrefireDecision {
    Passed(AuthenticatedQuote),
    Skipped,
    FailOpen(String),
    Abort(String),
}

fn env_bool(name: &str, default: bool) -> bool {
    std::env::var(name)
        .map(|v| !matches!(v.to_ascii_lowercase().as_str(), "0" | "false" | "no"))
        .unwrap_or(default)
}

fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|v| *v > 0)
        .unwrap_or(default)
}

fn env_optional_f64(name: &str) -> Option<f64> {
    std::env::var(name).ok().and_then(|v| v.parse().ok())
}

fn fill_timeout_ms(venue: &str) -> u64 {
    if venue == "pinnacle" {
        env_u64("PIN_FILL_TIMEOUT_MS", DEFAULT_PIN_FILL_TIMEOUT_MS)
    } else {
        env_u64("FILL_TIMEOUT_MS", DEFAULT_FILL_TIMEOUT_MS)
    }
}

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn emit_attempt(event_type: &str, attempt_id: &str, payload: Value) {
    if let Err(e) = attempt_telemetry::emit_event(event_type, Some(attempt_id), payload) {
        tracing::debug!("attempt telemetry write failed: {}", e);
    }
}

async fn send_json(writer: &WsWriter, value: Value) -> Result<(), String> {
    let payload = value.to_string();
    let mut w = writer.lock().await;
    w.send(Message::Text(payload))
        .await
        .map_err(|e| format!("WS send failed: {}", e))
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
    send_json(writer, json!({"type": "get_balances"})).await?;

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

async fn quote_leg_with_timeout_ms(
    writer: &WsWriter,
    reader: &WsReader,
    attempt_id: &str,
    venue: &str,
    venue_market_id: &str,
    timeout_ms: u64,
) -> Result<(f64, f64, f64), String> {
    send_json(
        writer,
        json!({
            "type": "quote_bet",
            "attempt_id": attempt_id,
            "venue": venue,
            "venue_market_id": venue_market_id,
        }),
    )
    .await?;

    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_millis(timeout_ms);
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Err(format!("quote timeout after {}ms", timeout_ms));
        }
        let msg = {
            let mut r = reader.lock().await;
            match tokio::time::timeout(remaining, r.next()).await {
                Ok(Some(Ok(Message::Text(text)))) => Some(text),
                Ok(Some(Ok(_))) => continue,
                Ok(Some(Err(e))) => return Err(format!("WS read error: {}", e)),
                Ok(None) => return Err("WS connection closed".to_string()),
                Err(_) => return Err(format!("quote timeout after {}ms", timeout_ms)),
            }
        };
        if let Some(text) = msg {
            if let Ok(q) = serde_json::from_str::<QuoteResult>(&text) {
                if q.msg_type.as_deref() == Some("quote_result") && q.attempt_id == attempt_id {
                    if q.status != "ok" {
                        let err = q.error_message.or(q.error).unwrap_or_default();
                        return Err(format!("quote failed: {}", err));
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

/// Pre-flight quote for Pinnacle. Returns min/max stake and current price.
pub async fn quote_leg(
    writer: &WsWriter,
    reader: &WsReader,
    attempt_id: &str,
    venue: &str,
    venue_market_id: &str,
) -> Result<(f64, f64, f64), String> {
    let timeout_ms = env_u64("QUOTE_TIMEOUT_MS", 10000);
    quote_leg_with_timeout_ms(writer, reader, attempt_id, venue, venue_market_id, timeout_ms).await
}

async fn validate_soft_book(
    writer: &WsWriter,
    reader: &WsReader,
    attempt_id: &str,
    venue: &str,
    venue_market_id: &str,
    stake: f64,
    expected_odds: f64,
    opposing_odds: f64,
) -> Result<f64, String> {
    if venue == "pinnacle" || !env_bool("SOFT_PREFIRE_VALIDATION_ENABLED", true) {
        return Ok(expected_odds);
    }

    let timeout_ms = env_u64("SOFT_PREFIRE_VALIDATION_TIMEOUT_MS", 3500);
    let max_slippage_pct = std::env::var("SOFT_PREFIRE_MAX_SLIPPAGE_PCT")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(0.07);

    send_json(
        writer,
        json!({
            "type": "validate_soft",
            "attempt_id": attempt_id,
            "venue": venue,
            "venue_market_id": venue_market_id,
            "stake": stake,
            "expected_odds": expected_odds,
            "max_slippage_pct": max_slippage_pct,
        }),
    )
    .await?;

    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_millis(timeout_ms);
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Err(format!("validate_soft timeout after {}ms", timeout_ms));
        }

        let msg = {
            let mut r = reader.lock().await;
            match tokio::time::timeout(remaining, r.next()).await {
                Ok(Some(Ok(Message::Text(text)))) => Some(text),
                Ok(Some(Ok(_))) => continue,
                Ok(Some(Err(e))) => return Err(format!("WS read error: {}", e)),
                Ok(None) => return Err("WS connection closed".to_string()),
                Err(_) => return Err(format!("validate_soft timeout after {}ms", timeout_ms)),
            }
        };

        if let Some(text) = msg {
            if let Ok(resp) = serde_json::from_str::<ValidateSoftResult>(&text) {
                if resp.msg_type.as_deref() == Some("validate_soft_result")
                    && resp.attempt_id == attempt_id
                {
                    if resp.status != "ok" {
                        return Err(format!(
                            "validate_soft failed: {}{}",
                            resp.error_message.unwrap_or_default(),
                            resp.error_code
                                .map(|c| format!(" ({c})"))
                                .unwrap_or_default()
                        ));
                    }

                    let fresh_price = resp
                        .price
                        .filter(|p| p.is_finite() && *p > 1.0)
                        .ok_or_else(|| "validate_soft returned invalid price".to_string())?;
                    let sigma = 1.0 / fresh_price + 1.0 / opposing_odds;
                    if sigma >= 1.0 {
                        return Err(format!(
                            "validate_soft killed arb: sigma={:.4} soft={:.3} hedge={:.3}",
                            sigma, fresh_price, opposing_odds
                        ));
                    }
                    tracing::info!(
                        "[SOFT VALIDATED] {} {} @ {:.3} sigma={:.4}",
                        venue,
                        resp.display_odds
                            .as_ref()
                            .map(|v| v.to_string())
                            .unwrap_or_else(|| venue_market_id.to_string()),
                        fresh_price,
                        sigma
                    );
                    return Ok(fresh_price);
                }
            }
        }
    }
}

async fn pin_prefire_recheck(
    writer: &WsWriter,
    reader: &WsReader,
    attempt_id: &str,
    pin_venue_market_id: &str,
    soft_odds: f64,
) -> PinPrefireDecision {
    if !env_bool("PIN_PREFIRE_RECHECK_ENABLED", true) {
        return PinPrefireDecision::Skipped;
    }

    let timeout_ms = env_u64("PIN_PREFIRE_RECHECK_TIMEOUT_MS", 1500);
    match quote_leg_with_timeout_ms(
        writer,
        reader,
        attempt_id,
        "pinnacle",
        pin_venue_market_id,
        timeout_ms,
    )
    .await
    {
        Ok((min_stake, max_stake, price)) => {
            let quote = match AuthenticatedQuote::new(price, min_stake, max_stake) {
                Ok(q) => q,
                Err(e) => return PinPrefireDecision::FailOpen(e),
            };
            let sigma = 1.0 / soft_odds + 1.0 / quote.price;
            if sigma >= 1.0 {
                return PinPrefireDecision::Abort(format!(
                    "pin_drift_killed_arb sigma={:.4} soft={:.3} pin={:.3}",
                    sigma, soft_odds, quote.price
                ));
            }
            PinPrefireDecision::Passed(quote)
        }
        Err(e) if e.contains("quote timeout") => PinPrefireDecision::FailOpen(e),
        Err(e) if pin_yank::is_pin_hard_yank_error(&e) => {
            pin_yank::record_pin_yank(pin_venue_market_id, &e);
            PinPrefireDecision::Abort(format!("pin_prefire_offline: {e}"))
        }
        Err(e) => PinPrefireDecision::FailOpen(e),
    }
}

/// Drain any pending balance_update messages from the WS and update the DB.
/// Non-blocking: reads for up to 500ms then returns.
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
                        update.venue,
                        update.balance
                    );
                    db.update_balance(&update.venue, update.balance);
                }
            }
        }
    }
}

/// Place a single leg via WebSocket command to the bet placer.
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

    let payload = serde_json::to_value(&cmd).map_err(|e| format!("serialize: {}", e))?;

    tracing::info!(
        venue = %venue,
        stake = stake,
        selection = %selection,
        "Sending PLACE_BET via WS"
    );

    send_json(writer, payload).await?;

    let timeout_ms = fill_timeout_ms(venue);
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_millis(timeout_ms);

    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Err(format!("fill timeout after {}ms", timeout_ms));
        }

        let msg = {
            let mut r = reader.lock().await;
            match tokio::time::timeout(remaining, r.next()).await {
                Ok(Some(Ok(Message::Text(text)))) => Some(text),
                Ok(Some(Ok(_))) => continue,
                Ok(Some(Err(e))) => return Err(format!("WS read error: {}", e)),
                Ok(None) => return Err("WS connection closed".to_string()),
                Err(_) => return Err(format!("fill timeout after {}ms", timeout_ms)),
            }
        };

        if let Some(text) = msg {
            if let Ok(fill) = serde_json::from_str::<FillReport>(&text) {
                if fill.msg_type.as_deref() == Some("fill_report") && fill.attempt_id == attempt_id
                {
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

async fn get_bet_status(
    writer: &WsWriter,
    reader: &WsReader,
    attempt_id: &str,
    venue: &str,
    purchase_id: Option<&str>,
    bet_id: Option<&str>,
) -> Result<BetStatusResult, String> {
    send_json(
        writer,
        json!({
            "type": "get_bet_status",
            "attempt_id": attempt_id,
            "venue": venue,
            "purchase_id": purchase_id,
            "bet_id": bet_id,
        }),
    )
    .await?;

    let timeout_ms = env_u64("BET_STATUS_TIMEOUT_MS", 5000);
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_millis(timeout_ms);
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Err(format!("bet status timeout after {}ms", timeout_ms));
        }

        let msg = {
            let mut r = reader.lock().await;
            match tokio::time::timeout(remaining, r.next()).await {
                Ok(Some(Ok(Message::Text(text)))) => Some(text),
                Ok(Some(Ok(_))) => continue,
                Ok(Some(Err(e))) => return Err(format!("WS read error: {}", e)),
                Ok(None) => return Err("WS connection closed".to_string()),
                Err(_) => return Err(format!("bet status timeout after {}ms", timeout_ms)),
            }
        };

        if let Some(text) = msg {
            if let Ok(status) = serde_json::from_str::<BetStatusResult>(&text) {
                if status.msg_type.as_deref() == Some("bet_status")
                    && status.attempt_id.as_deref() == Some(attempt_id)
                {
                    return Ok(status);
                }
            }
        }
    }
}

async fn cashout_bet(
    writer: &WsWriter,
    reader: &WsReader,
    attempt_id: &str,
    venue: &str,
    purchase_id: Option<&str>,
    bet_id: Option<&str>,
    amount: Option<f64>,
) -> Result<CashoutResult, String> {
    let id_for_cashout = purchase_id.or(bet_id);
    send_json(
        writer,
        json!({
            "type": "cashout",
            "attempt_id": attempt_id,
            "venue": venue,
            "purchase_id": purchase_id,
            "bet_id": id_for_cashout,
            "dk_bet_id": bet_id,
            "amount": amount,
        }),
    )
    .await?;

    let timeout_ms = env_u64("CASHOUT_TIMEOUT_MS", 15000);
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_millis(timeout_ms);
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Err(format!("cashout timeout after {}ms", timeout_ms));
        }

        let msg = {
            let mut r = reader.lock().await;
            match tokio::time::timeout(remaining, r.next()).await {
                Ok(Some(Ok(Message::Text(text)))) => Some(text),
                Ok(Some(Ok(_))) => continue,
                Ok(Some(Err(e))) => return Err(format!("WS read error: {}", e)),
                Ok(None) => return Err("WS connection closed".to_string()),
                Err(_) => return Err(format!("cashout timeout after {}ms", timeout_ms)),
            }
        };

        if let Some(text) = msg {
            if let Ok(result) = serde_json::from_str::<CashoutResult>(&text) {
                if result.msg_type.as_deref() == Some("cashout_result")
                    && result.attempt_id.as_deref() == Some(attempt_id)
                {
                    return Ok(result);
                }
            }
        }
    }
}

fn build_pin_rescue_market_ids(pin_venue_market_id: &str) -> Vec<String> {
    let mut markets = vec![pin_venue_market_id.to_string()];
    let parts: Vec<&str> = pin_venue_market_id.split(':').collect();
    if parts.len() < 3 {
        return markets;
    }

    let matchup = parts[0];
    let designation = parts[parts.len() - 1];
    let market_key = parts[1..parts.len() - 1].join(":");
    let pieces: Vec<&str> = market_key.split(';').collect();

    if pieces.len() == 4 && pieces[0] == "s" && pieces[2] == "ou" {
        if let Ok(original) = pieces[3].parse::<f64>() {
            for offset in [0.5, -0.5, 1.0, -1.0, 1.5, -1.5, 2.0, -2.0] {
                let line = original + offset;
                if line <= 0.0 {
                    continue;
                }
                let id = format!("{matchup}:s;{};ou;{:.1}:{designation}", pieces[1], line);
                if !markets.contains(&id) {
                    markets.push(id);
                }
            }
        }
    } else if pieces.len() == 4 && pieces[0] == "s" && pieces[2] == "s" {
        if let Ok(original) = pieces[3].parse::<f64>() {
            for offset in [0.5, -0.5, 1.0, -1.0, 1.5, -1.5] {
                let line = original + offset;
                let id = format!("{matchup}:s;{};s;{:.1}:{designation}", pieces[1], line);
                if !markets.contains(&id) {
                    markets.push(id);
                }
            }
        }
    } else if pieces.len() == 5 && pieces[0] == "s" && pieces[2] == "tt" {
        if let Ok(original) = pieces[3].parse::<f64>() {
            for offset in [0.5, -0.5, 1.0, -1.0, 1.5, -1.5, 2.0, -2.0] {
                let line = original + offset;
                if line <= 0.0 {
                    continue;
                }
                let id = format!(
                    "{matchup}:s;{};tt;{:.1};{}:{designation}",
                    pieces[1], line, pieces[4]
                );
                if !markets.contains(&id) {
                    markets.push(id);
                }
            }
        }
    }

    markets
}

async fn attempt_recovery(
    writer: &WsWriter,
    reader: &WsReader,
    db: &Arc<Db>,
    attempt_id: &str,
    candidate: &ArbCandidate,
    _settings: &Settings,
    leg1_venue: &str,
    leg1_market_id: &str,
    leg1_selection: &str,
    leg1_stake: f64,
    leg1_odds: f64,
    leg1_fill: &FillReport,
    pin_venue_market_id: Option<&str>,
    pin_selection: Option<&str>,
    initial_error: &str,
) {
    emit_attempt(
        "RESCUE_ENTER",
        attempt_id,
        json!({
            "leg1_venue": leg1_venue,
            "leg1_market_id": leg1_market_id,
            "stake": leg1_stake,
            "odds": leg1_odds,
            "pin_venue_market_id": pin_venue_market_id,
            "initial_error": initial_error,
        }),
    );

    let purchase_id = leg1_fill.purchase_id.as_deref();
    let bet_id = leg1_fill.bet_id.as_deref();
    let mut cashout_offer = None;

    if purchase_id.is_some() || bet_id.is_some() {
        match get_bet_status(
            writer,
            reader,
            &format!("{attempt_id}-STATUS0"),
            leg1_venue,
            purchase_id,
            bet_id,
        )
        .await
        {
            Ok(status) => {
                cashout_offer = status.cashout_offer;
                emit_attempt(
                    "NAKED_LEG_STATUS_CHECKED",
                    attempt_id,
                    json!({
                        "venue": leg1_venue,
                        "status": status.status,
                        "raw_status": status.raw_status,
                        "cashout_available": status.cashout_available,
                        "cashout_offer": status.cashout_offer,
                        "settled_at": status.settled_at,
                        "outcome": status.outcome,
                        "response_venue": status.venue,
                        "purchase_id": status.purchase_id,
                        "bet_id": status.bet_id,
                    }),
                );
            }
            Err(e) => {
                emit_attempt("NAKED_LEG_STATUS_ERROR", attempt_id, json!({ "error": e }));
            }
        }
    }

    let mut rescued = false;
    if let (Some(pin_market), Some(pin_sel)) = (pin_venue_market_id, pin_selection) {
        let markets_to_try = build_pin_rescue_market_ids(pin_market);
        let threshold = env_u64("RESCUE_PIN_HARD_YANK_THRESHOLD", 3).max(1);
        let mut hard_yank_count = if pin_yank::is_pin_hard_yank_error(initial_error) {
            1
        } else {
            0
        };
        let rescue_timeout_ms = env_u64("RESCUE_QUOTE_TIMEOUT_MS", 3000);
        let max_rescue_loss = env_optional_f64("MAX_RESCUE_LOSS");

        for (idx, try_id) in markets_to_try.iter().enumerate() {
            if hard_yank_count >= threshold {
                emit_attempt(
                    "RESCUE_SKIP",
                    attempt_id,
                    json!({
                        "reason": "pin_hard_yank_threshold",
                        "threshold": threshold,
                        "hard_yank_count": hard_yank_count,
                        "remaining": markets_to_try.len().saturating_sub(idx),
                    }),
                );
                break;
            }

            let quote_result = quote_leg_with_timeout_ms(
                writer,
                reader,
                &format!("{attempt_id}-RQ{idx}"),
                "pinnacle",
                try_id,
                rescue_timeout_ms,
            )
            .await;

            let quote = match quote_result {
                Ok((min_stake, max_stake, price)) => {
                    match AuthenticatedQuote::new(price, min_stake, max_stake) {
                        Ok(q) => q,
                        Err(e) => {
                            emit_attempt("RESCUE_QUOTE_INVALID", attempt_id, json!({ "market": try_id, "error": e }));
                            continue;
                        }
                    }
                }
                Err(e) => {
                    if pin_yank::is_pin_hard_yank_error(&e) {
                        hard_yank_count += 1;
                        pin_yank::record_pin_yank(try_id, &e);
                    }
                    emit_attempt(
                        "RESCUE_QUOTE_FAIL",
                        attempt_id,
                        json!({
                            "market": try_id,
                            "error": e,
                            "hard_yank_count": hard_yank_count,
                            "threshold": threshold,
                        }),
                    );
                    continue;
                }
            };

            let Some(hedge) = compute_pin_hedge_from_soft_fill(leg1_stake, leg1_odds, quote) else {
                emit_attempt("RESCUE_MATH_FAIL", attempt_id, json!({ "market": try_id }));
                continue;
            };

            let pin_balance = db.get_balance("pinnacle").unwrap_or(0.0);
            if hedge.pin_stake > pin_balance {
                emit_attempt(
                    "RESCUE_PIN_BALANCE_INSUFFICIENT",
                    attempt_id,
                    json!({
                        "market": try_id,
                        "stake": hedge.pin_stake,
                        "pin_balance": pin_balance,
                    }),
                );
                continue;
            }

            let rescue_loss = (-hedge.guaranteed_profit).max(0.0);
            if max_rescue_loss.is_some_and(|limit| rescue_loss > limit) {
                emit_attempt(
                    "RESCUE_LOSS_ABOVE_MAX",
                    attempt_id,
                    json!({ "market": try_id, "loss": rescue_loss, "limit": max_rescue_loss }),
                );
                continue;
            }

            if let Some(offer) = cashout_offer {
                let cashout_loss = (leg1_stake - offer).max(0.0);
                if cashout_loss <= rescue_loss {
                    emit_attempt(
                        "RESCUE_SKIPPED_CASHOUT_CHEAPER",
                        attempt_id,
                        json!({
                            "market": try_id,
                            "rescue_loss": rescue_loss,
                            "cashout_loss": cashout_loss,
                            "cashout_offer": offer,
                        }),
                    );
                    break;
                }
            }

            emit_attempt(
                "RESCUE_PLACE_SENT",
                attempt_id,
                json!({
                    "market": try_id,
                    "stake": hedge.pin_stake,
                    "odds": quote.price,
                    "rescue_loss": rescue_loss,
                    "sigma": hedge.sigma,
                }),
            );

            match place_leg(
                writer,
                reader,
                &format!("{attempt_id}-R{idx}"),
                "pinnacle",
                try_id,
                pin_sel,
                hedge.pin_stake,
                quote.price,
            )
            .await
            {
                Ok(fill) if fill.status == "filled" => {
                    drain_balance_updates(reader, db).await;
                    emit_attempt(
                        "RESCUE_FILLED",
                        attempt_id,
                        json!({
                            "market": try_id,
                            "accepted_stake": fill.accepted_stake,
                            "accepted_odds": fill.accepted_odds,
                        }),
                    );
                    rescued = true;
                    break;
                }
                Ok(fill) => {
                    let err = fill.error_message.unwrap_or_default();
                    if pin_yank::is_pin_hard_yank_error(&err) {
                        hard_yank_count += 1;
                        pin_yank::record_pin_yank(try_id, &err);
                    }
                    emit_attempt(
                        "RESCUE_REJECTED",
                        attempt_id,
                        json!({
                            "market": try_id,
                            "status": fill.status,
                            "error": err,
                            "hard_yank_count": hard_yank_count,
                            "threshold": threshold,
                        }),
                    );
                }
                Err(e) => {
                    if pin_yank::is_pin_hard_yank_error(&e) {
                        hard_yank_count += 1;
                        pin_yank::record_pin_yank(try_id, &e);
                    }
                    emit_attempt(
                        "RESCUE_ERROR",
                        attempt_id,
                        json!({
                            "market": try_id,
                            "error": e,
                            "hard_yank_count": hard_yank_count,
                            "threshold": threshold,
                        }),
                    );
                }
            }
        }
    }

    if rescued {
        return;
    }

    let mut cashout_resolved = false;
    if purchase_id.is_some() || bet_id.is_some() {
        match cashout_bet(
            writer,
            reader,
            &format!("{attempt_id}-CASHOUT"),
            leg1_venue,
            purchase_id,
            bet_id,
            cashout_offer,
        )
        .await
        {
            Ok(result) => {
                let class = classify_cashout_confirmation(
                    result.raw_status.as_deref(),
                    None,
                    result.error.as_deref(),
                );
                emit_attempt(
                    "CASHOUT_RESULT",
                    attempt_id,
                    json!({
                        "success": result.success,
                        "confirmed": result.confirmed,
                        "unconfirmed": result.unconfirmed,
                        "submitted": result.submitted,
                        "terminal_non_cashout": result.terminal_non_cashout,
                        "payout": result.payout,
                        "payout_claimed": result.payout_claimed,
                        "raw_status": result.raw_status,
                        "reason": result.reason,
                        "error": result.error,
                        "classification": format!("{:?}", class),
                    }),
                );
                cashout_resolved = matches!(
                    class,
                    CashoutConfirmation::ConfirmedCashout | CashoutConfirmation::TerminalNonCashout
                ) || (result.success == Some(true) && result.confirmed == Some(true));
            }
            Err(e) => emit_attempt("CASHOUT_ERROR", attempt_id, json!({ "error": e })),
        }
    }

    if cashout_resolved {
        return;
    }

    let leg = naked_legs::new_naked_leg(
        attempt_id,
        leg1_venue,
        leg1_market_id,
        leg1_selection,
        leg1_stake,
        leg1_odds,
        leg1_fill.purchase_id.clone(),
        leg1_fill.bet_id.clone(),
        &candidate.event_id,
        &candidate.sport,
        &candidate.market_type,
        pin_venue_market_id.map(|s| s.to_string()),
        pin_selection.map(|s| s.to_string()),
        Some(initial_error.to_string()),
    );

    if let Err(e) = naked_legs::add_naked_leg(leg.clone()) {
        tracing::error!("Failed to persist naked leg: {}", e);
    }
    emit_attempt(
        "NAKED_LEG",
        attempt_id,
        json!({
            "id": leg.id,
            "venue": leg.venue,
            "stake": leg.stake,
            "odds": leg.odds,
            "purchase_id": leg.purchase_id,
            "bet_id": leg.bet_id,
            "pin_venue_market_id": leg.pin_venue_market_id,
        }),
    );
}

/// Reload and monitor persisted naked legs. This keeps unresolved exposure alive
/// across restarts without changing the terminal-first operator surface.
pub async fn monitor_naked_legs(
    writer: &WsWriter,
    reader: &WsReader,
    pin_balance: f64,
) -> Result<(), String> {
    let legs = naked_legs::load_naked_legs();
    if legs.is_empty() {
        return Ok(());
    }

    tracing::info!("[NAKED LEG] monitoring {} persisted leg(s)", legs.len());
    let mut remaining = Vec::new();

    for mut leg in legs {
        let mut resolved = false;
        let mut cashout_offer = None;

        if leg.purchase_id.is_some() || leg.bet_id.is_some() {
            match get_bet_status(
                writer,
                reader,
                &format!("{}-MONS", leg.attempt_id),
                &leg.venue,
                leg.purchase_id.as_deref(),
                leg.bet_id.as_deref(),
            )
            .await
            {
                Ok(status) => {
                    let class = classify_cashout_confirmation(
                        status.raw_status.as_deref(),
                        status.status.as_deref(),
                        status.error.as_deref(),
                    );
                    cashout_offer = status.cashout_offer;
                    emit_attempt(
                        "NAKED_LEG_STATUS_CHECKED",
                        &leg.attempt_id,
                        json!({
                            "status": status.status,
                            "raw_status": status.raw_status,
                            "classification": format!("{:?}", class),
                            "cashout_offer": status.cashout_offer,
                            "settled_at": status.settled_at,
                            "outcome": status.outcome,
                            "response_venue": status.venue,
                            "purchase_id": status.purchase_id,
                            "bet_id": status.bet_id,
                        }),
                    );

                    if matches!(
                        class,
                        CashoutConfirmation::ConfirmedCashout
                            | CashoutConfirmation::TerminalNonCashout
                    ) {
                        resolved = true;
                    }
                }
                Err(e) => {
                    emit_attempt(
                        "NAKED_LEG_STATUS_ERROR",
                        &leg.attempt_id,
                        json!({ "error": e }),
                    );
                }
            }
        }

        if !resolved {
            if let (Some(pin_id), Some(pin_selection)) =
            (leg.pin_venue_market_id.clone(), leg.pin_selection.clone())
            {
                if !pin_yank::is_blacklisted(&pin_id) {
                    if let Ok((min_stake, max_quote_stake, price)) = quote_leg_with_timeout_ms(
                        writer,
                        reader,
                        &format!("{}-MONQ", leg.attempt_id),
                        "pinnacle",
                        &pin_id,
                        env_u64("NAKED_LEG_PIN_RECHECK_QUOTE_TIMEOUT_MS", 3000),
                    )
                    .await
                    {
                        if let Ok(quote) =
                            AuthenticatedQuote::new(price, min_stake, max_quote_stake)
                        {
                            if let Some(hedge) =
                                compute_pin_hedge_from_soft_fill(leg.stake, leg.odds, quote)
                            {
                                if hedge.pin_stake <= pin_balance {
                                    match place_leg(
                                        writer,
                                        reader,
                                        &format!("{}-MONR", leg.attempt_id),
                                        "pinnacle",
                                        &pin_id,
                                        &pin_selection,
                                        hedge.pin_stake,
                                        quote.price,
                                    )
                                    .await
                                    {
                                        Ok(fill) if fill.status == "filled" => {
                                            emit_attempt(
                                                "NAKED_LEG_PIN_RELIST_RESCUED",
                                                &leg.attempt_id,
                                                json!({
                                                    "market": pin_id,
                                                    "stake": fill.accepted_stake,
                                                    "odds": fill.accepted_odds,
                                                }),
                                            );
                                            resolved = true;
                                        }
                                        Ok(fill) => {
                                            if let Some(err) = fill.error_message.as_deref() {
                                                if pin_yank::is_pin_hard_yank_error(err) {
                                                    pin_yank::record_pin_yank(&pin_id, err);
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            if pin_yank::is_pin_hard_yank_error(&e) {
                                                pin_yank::record_pin_yank(&pin_id, &e);
                                            }
                                        }
                                    }
                                } else {
                                    emit_attempt(
                                        "NAKED_LEG_PIN_BALANCE_INSUFFICIENT",
                                        &leg.attempt_id,
                                        json!({
                                            "market": pin_id,
                                            "stake": hedge.pin_stake,
                                            "pin_balance": pin_balance,
                                        }),
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }

        if !resolved && cashout_offer.is_some() && (leg.purchase_id.is_some() || leg.bet_id.is_some())
        {
            match cashout_bet(
                writer,
                reader,
                &format!("{}-MONC", leg.attempt_id),
                &leg.venue,
                leg.purchase_id.as_deref(),
                leg.bet_id.as_deref(),
                cashout_offer,
            )
            .await
            {
                Ok(result) => {
                    leg.cashout_attempts += 1;
                    let class = classify_cashout_confirmation(
                        result.raw_status.as_deref(),
                        None,
                        result.error.as_deref(),
                    );
                    emit_attempt(
                        "NAKED_LEG_CASHOUT_RESULT",
                        &leg.attempt_id,
                        json!({
                            "success": result.success,
                            "confirmed": result.confirmed,
                            "raw_status": result.raw_status,
                            "classification": format!("{:?}", class),
                        }),
                    );
                    if matches!(
                        class,
                        CashoutConfirmation::ConfirmedCashout
                            | CashoutConfirmation::TerminalNonCashout
                    ) || (result.success == Some(true) && result.confirmed == Some(true))
                    {
                        resolved = true;
                    }
                }
                Err(e) => {
                    emit_attempt(
                        "NAKED_LEG_CASHOUT_ERROR",
                        &leg.attempt_id,
                        json!({ "error": e }),
                    );
                }
            }
        }

        if resolved {
            let _ = naked_legs::remove_naked_leg(&leg.id);
            emit_attempt("NAKED_LEG_RESOLVED", &leg.attempt_id, json!({ "id": leg.id }));
        } else {
            leg.updated_at = now_iso();
            leg.last_checked_at = Some(now_iso());
            remaining.push(leg);
        }
    }

    naked_legs::save_naked_legs(&remaining).map_err(|e| e.to_string())?;
    Ok(())
}

/// Venues we can actually place bets on via Patchright.
const BETTABLE_VENUES: &[&str] = &["fanduel", "draftkings", "pinnacle"];

/// Live execution: size the trade, validate the soft side, place soft leg first,
/// then hedge. If hedge fails after soft fill, enter recovery instead of
/// forgetting the exposure.
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
            candidate.leg1_venue,
            candidate.leg2_venue
        );
        return None;
    }

    let min_arb_pct = settings.min_arb_pct / 100.0;
    if candidate.arb_pct < min_arb_pct {
        return None;
    }

    let leg1_balance = db.get_balance(&candidate.leg1_venue)?;
    let leg2_balance = db.get_balance(&candidate.leg2_venue)?;

    let requested_capital = stake_optimizer::requested_capital_with_soft_cap(
        candidate,
        settings,
        leg1_balance,
        leg2_balance,
    );

    if requested_capital <= 0.0 {
        return None;
    }

    let (plan, effective_capital, was_scaled) = stake_optimizer::compute_stakes_with_balances(
        candidate,
        requested_capital,
        leg1_balance,
        leg2_balance,
        settings.max_stake,
    )?;

    if was_scaled {
        tracing::info!(
            "[SCALED] Capital ${:.2} -> ${:.2} (balance/soft max constraint)",
            requested_capital,
            effective_capital
        );
    }

    let attempt_id = uuid::Uuid::new_v4().to_string();
    let mut authenticated_pin_quote: Option<AuthenticatedQuote> = None;

    emit_attempt(
        "FIRE_ENTER",
        &attempt_id,
        json!({
            "event_id": candidate.event_id,
            "sport": candidate.sport,
            "market_type": candidate.market_type,
            "period": candidate.period,
            "line": candidate.line,
            "arb_pct": candidate.arb_pct,
            "sigma": candidate.sigma,
        }),
    );

    let leg1_is_first = if candidate.leg1_venue == "pinnacle" {
        false
    } else if candidate.leg2_venue == "pinnacle" {
        true
    } else if candidate.leg1_venue == "fanduel" {
        true
    } else {
        false
    };

    let (
        first_venue,
        first_sel,
        mut first_stake,
        mut first_odds,
        first_mkt_id,
        second_venue,
        second_sel,
        mut second_stake,
        mut second_odds,
        second_mkt_id,
    ) = if leg1_is_first {
        (
            &candidate.leg1_venue,
            &candidate.leg1_selection,
            plan.leg1_stake,
            candidate.leg1_odds,
            candidate.leg1_venue_market_id.clone(),
            &candidate.leg2_venue,
            &candidate.leg2_selection,
            plan.leg2_stake,
            candidate.leg2_odds,
            candidate.leg2_venue_market_id.clone(),
        )
    } else {
        (
            &candidate.leg2_venue,
            &candidate.leg2_selection,
            plan.leg2_stake,
            candidate.leg2_odds,
            candidate.leg2_venue_market_id.clone(),
            &candidate.leg1_venue,
            &candidate.leg1_selection,
            plan.leg1_stake,
            candidate.leg1_odds,
            candidate.leg1_venue_market_id.clone(),
        )
    };
    let (first_balance, second_balance) = if leg1_is_first {
        (leg1_balance, leg2_balance)
    } else {
        (leg2_balance, leg1_balance)
    };

    let has_pinnacle = first_venue == "pinnacle" || second_venue == "pinnacle";
    if has_pinnacle {
        let pin_mkt_id = if first_venue == "pinnacle" {
            &first_mkt_id
        } else {
            &second_mkt_id
        };

        if pin_yank::is_blacklisted(pin_mkt_id) {
            emit_attempt(
                "PIN_YANK_BLACKLIST_SKIP",
                &attempt_id,
                json!({ "pin_venue_market_id": pin_mkt_id }),
            );
            return None;
        }

        let (pin_venue, pin_mkt_id, soft_stake, soft_odds) = if first_venue == "pinnacle" {
            (first_venue.as_str(), &first_mkt_id, second_stake, second_odds)
        } else {
            (second_venue.as_str(), &second_mkt_id, first_stake, first_odds)
        };

        match quote_leg(writer, reader, &format!("{}-Q", attempt_id), pin_venue, pin_mkt_id).await {
            Ok((min_stake, max_stake, quoted_price)) => {
                let quote = match AuthenticatedQuote::new(quoted_price, min_stake, max_stake) {
                    Ok(q) => q,
                    Err(e) => {
                        tracing::info!("[PIN QUOTE] Invalid authenticated quote: {}", e);
                        emit_attempt("PIN_QUOTE_INVALID", &attempt_id, json!({ "error": e }));
                        return None;
                    }
                };

                let hedge = match compute_pin_hedge_from_soft_fill(soft_stake, soft_odds, quote) {
                    Some(h) => h,
                    None => {
                        tracing::info!("[PIN QUOTE] Could not compute hedge from authenticated quote");
                        emit_attempt("PIN_QUOTE_MATH_FAILED", &attempt_id, json!({}));
                        return None;
                    }
                };

                if hedge.sigma >= 1.0 || hedge.guaranteed_profit < -0.0001 {
                    tracing::info!(
                        "[PIN QUOTE] Arb closed after authenticated quote: sigma={:.4} profit=${:.2} (soft@{:.3}, PIN@{:.3})",
                        hedge.sigma,
                        hedge.guaranteed_profit,
                        soft_odds,
                        quote.price
                    );
                    emit_attempt(
                        "PIN_QUOTE_KILLED_ARB",
                        &attempt_id,
                        json!({ "sigma": hedge.sigma, "profit": hedge.guaranteed_profit }),
                    );
                    return None;
                }

                let pin_balance = if first_venue == "pinnacle" {
                    first_balance
                } else {
                    second_balance
                };
                if hedge.pin_stake > pin_balance {
                    tracing::info!(
                        "[PIN QUOTE] PIN stake ${:.2} exceeds balance ${:.2}; skipping",
                        hedge.pin_stake,
                        pin_balance
                    );
                    emit_attempt(
                        "PIN_QUOTE_BALANCE_INSUFFICIENT",
                        &attempt_id,
                        json!({ "pin_stake": hedge.pin_stake, "pin_balance": pin_balance }),
                    );
                    return None;
                }

                if first_venue == "pinnacle" {
                    first_stake = hedge.pin_stake;
                    first_odds = quote.price;
                } else {
                    second_stake = hedge.pin_stake;
                    second_odds = quote.price;
                }

                authenticated_pin_quote = Some(quote);
                emit_attempt(
                    "PIN_QUOTED",
                    &attempt_id,
                    json!({
                        "pin_venue_market_id": pin_mkt_id,
                        "price": quote.price,
                        "min_stake": quote.min_stake,
                        "max_stake": quote.max_stake,
                        "pin_stake": hedge.pin_stake,
                        "sigma": hedge.sigma,
                        "profit": hedge.guaranteed_profit,
                    }),
                );
                tracing::info!(
                    "[PIN QUOTE] Authenticated hedge truth: PIN@{:.3} stake=${:.2} min=${:.2} max=${:.2} sigma={:.4} profit=${:.2}",
                    quote.price,
                    hedge.pin_stake,
                    quote.min_stake,
                    quote.max_stake,
                    hedge.sigma,
                    hedge.guaranteed_profit
                );
            }
            Err(e) => {
                if pin_yank::is_pin_hard_yank_error(&e) {
                    pin_yank::record_pin_yank(pin_mkt_id, &e);
                }
                tracing::error!("[QUOTE] Failed: {}. Skipping arb.", e);
                emit_attempt("PIN_QUOTE_FAILED", &attempt_id, json!({ "error": e }));
                return None;
            }
        }
    }

    if first_venue != "pinnacle" {
        match validate_soft_book(
            writer,
            reader,
            &format!("{}-VS", attempt_id),
            first_venue,
            &first_mkt_id,
            first_stake,
            first_odds,
            second_odds,
        )
        .await
        {
            Ok(fresh_soft_odds) => {
                first_odds = fresh_soft_odds;
                emit_attempt(
                    "SOFT_VALIDATED",
                    &attempt_id,
                    json!({
                        "venue": first_venue,
                        "venue_market_id": first_mkt_id,
                        "fresh_odds": fresh_soft_odds,
                    }),
                );

                if second_venue == "pinnacle" {
                    if let Some(quote) = authenticated_pin_quote {
                        let Some(hedge) =
                            compute_pin_hedge_from_soft_fill(first_stake, first_odds, quote)
                        else {
                            emit_attempt("SOFT_VALIDATED_HEDGE_MATH_FAILED", &attempt_id, json!({}));
                            return None;
                        };
                        if hedge.sigma >= 1.0 || hedge.guaranteed_profit < -0.0001 {
                            emit_attempt(
                                "SOFT_VALIDATION_KILLED_ARB",
                                &attempt_id,
                                json!({ "sigma": hedge.sigma, "profit": hedge.guaranteed_profit }),
                            );
                            return None;
                        }
                        if hedge.pin_stake > second_balance {
                            emit_attempt(
                                "SOFT_VALIDATED_PIN_BALANCE_INSUFFICIENT",
                                &attempt_id,
                                json!({ "pin_stake": hedge.pin_stake, "pin_balance": second_balance }),
                            );
                            return None;
                        }
                        second_stake = hedge.pin_stake;
                        second_odds = quote.price;
                    }
                }
            }
            Err(e) => {
                tracing::info!("[SOFT VALIDATION] {}", e);
                emit_attempt("SOFT_VALIDATION_FAILED", &attempt_id, json!({ "error": e }));
                return None;
            }
        }
    }

    if second_venue == "pinnacle" {
        match pin_prefire_recheck(
            writer,
            reader,
            &format!("{}-PREFIRE", attempt_id),
            &second_mkt_id,
            first_odds,
        )
        .await
        {
            PinPrefireDecision::Passed(quote) => {
                if let Some(hedge) = compute_pin_hedge_from_soft_fill(first_stake, first_odds, quote)
                {
                    if hedge.pin_stake > second_balance {
                        emit_attempt(
                            "PIN_PREFIRE_BALANCE_INSUFFICIENT",
                            &attempt_id,
                            json!({ "pin_stake": hedge.pin_stake, "pin_balance": second_balance }),
                        );
                        return None;
                    }
                    authenticated_pin_quote = Some(quote);
                    second_stake = hedge.pin_stake;
                    second_odds = quote.price;
                    emit_attempt(
                        "PIN_PREFIRE_RECHECK_PASSED",
                        &attempt_id,
                        json!({
                            "pin_venue_market_id": second_mkt_id,
                            "price": quote.price,
                            "pin_stake": hedge.pin_stake,
                            "sigma": hedge.sigma,
                            "profit": hedge.guaranteed_profit,
                        }),
                    );
                } else {
                    emit_attempt("PIN_PREFIRE_RECHECK_MATH_FAILED", &attempt_id, json!({}));
                    return None;
                }
            }
            PinPrefireDecision::Abort(reason) => {
                emit_attempt(
                    "PIN_PREFIRE_RECHECK_ABORT",
                    &attempt_id,
                    json!({ "reason": reason }),
                );
                return None;
            }
            PinPrefireDecision::FailOpen(reason) => {
                emit_attempt(
                    "PIN_PREFIRE_RECHECK_FAIL_OPEN",
                    &attempt_id,
                    json!({ "reason": reason }),
                );
                tracing::warn!("[PIN PREFIRE] fail-open: {}", reason);
            }
            PinPrefireDecision::Skipped => {
                emit_attempt("PIN_PREFIRE_RECHECK_SKIPPED", &attempt_id, json!({}));
            }
        }
    }

    tracing::info!(
        "LEG 1: {} ${:.2} on {} @ {:.3}",
        first_venue,
        first_stake,
        first_sel,
        first_odds
    );
    emit_attempt(
        "LEG1_SENT",
        &attempt_id,
        json!({
            "venue": first_venue,
            "venue_market_id": first_mkt_id,
            "selection": first_sel,
            "stake": first_stake,
            "odds": first_odds,
        }),
    );

    let fill1 = match place_leg(
        writer,
        reader,
        &format!("{}-L1", attempt_id),
        first_venue,
        &first_mkt_id,
        first_sel,
        first_stake,
        first_odds,
    )
    .await
    {
        Ok(f) if f.status == "filled" => f,
        Ok(f) => {
            let err = f.error_message.unwrap_or_default();
            tracing::error!("LEG 1 FAILED: {} - {}.", f.status, err);
            emit_attempt("LEG1_FAILED", &attempt_id, json!({ "status": f.status, "error": err }));
            return None;
        }
        Err(e) => {
            tracing::error!("LEG 1 ERROR: {}.", e);
            emit_attempt("LEG1_ERROR", &attempt_id, json!({ "error": e }));
            return None;
        }
    };

    drain_balance_updates(reader, db).await;

    let s1 = fill1.accepted_stake.unwrap_or(first_stake);
    let o1 = fill1.accepted_odds.unwrap_or(first_odds);
    emit_attempt(
        "LEG1_FILLED",
        &attempt_id,
        json!({
            "accepted_stake": s1,
            "accepted_odds": o1,
            "purchase_id": fill1.purchase_id,
            "bet_id": fill1.bet_id,
            "raw_status": fill1.raw_status,
            "reconciliation_hint": fill1.reconciliation_hint,
        }),
    );

    if second_venue == "pinnacle" {
        if let Some(quote) = authenticated_pin_quote {
            let hedge = match compute_pin_hedge_from_soft_fill(s1, o1, quote) {
                Some(h) => h,
                None => {
                    let reason = "post_fill_pin_hedge_math_failed";
                    tracing::warn!("[POST-FILL] Could not recompute PIN hedge from actual soft fill");
                    attempt_recovery(
                        writer,
                        reader,
                        db,
                        &attempt_id,
                        candidate,
                        settings,
                        first_venue,
                        &first_mkt_id,
                        first_sel,
                        s1,
                        o1,
                        &fill1,
                        Some(&second_mkt_id),
                        Some(second_sel),
                        reason,
                    )
                    .await;
                    return None;
                }
            };

            if hedge.pin_stake > second_balance {
                let reason = format!(
                    "post_fill_pin_balance_insufficient stake={:.2} balance={:.2}",
                    hedge.pin_stake, second_balance
                );
                tracing::warn!("[POST-FILL] {}", reason);
                attempt_recovery(
                    writer,
                    reader,
                    db,
                    &attempt_id,
                    candidate,
                    settings,
                    first_venue,
                    &first_mkt_id,
                    first_sel,
                    s1,
                    o1,
                    &fill1,
                    Some(&second_mkt_id),
                    Some(second_sel),
                    &reason,
                )
                .await;
                return None;
            }

            second_stake = hedge.pin_stake;
            second_odds = quote.price;

            tracing::info!(
                "[POST-FILL] Recomputed PIN hedge from actual fill: soft ${:.2}@{:.3} -> PIN ${:.2}@{:.3} sigma={:.4} profit=${:.2}",
                s1,
                o1,
                second_stake,
                second_odds,
                hedge.sigma,
                hedge.guaranteed_profit
            );
            emit_attempt(
                "POST_FILL_HEDGE_RECOMPUTED",
                &attempt_id,
                json!({
                    "soft_stake": s1,
                    "soft_odds": o1,
                    "pin_stake": second_stake,
                    "pin_odds": second_odds,
                    "sigma": hedge.sigma,
                    "profit": hedge.guaranteed_profit,
                }),
            );

            if hedge.sigma >= 1.0 || hedge.guaranteed_profit < -0.0001 {
                let reason = format!(
                    "post_fill_arb_dead sigma={:.4} profit={:.2}",
                    hedge.sigma, hedge.guaranteed_profit
                );
                tracing::warn!("[POST-FILL] {}", reason);
                attempt_recovery(
                    writer,
                    reader,
                    db,
                    &attempt_id,
                    candidate,
                    settings,
                    first_venue,
                    &first_mkt_id,
                    first_sel,
                    s1,
                    o1,
                    &fill1,
                    Some(&second_mkt_id),
                    Some(second_sel),
                    &reason,
                )
                .await;
                return None;
            }
        } else {
            let reason = "post_fill_missing_authenticated_pin_quote";
            tracing::warn!("[POST-FILL] Missing authenticated PIN quote before hedge placement");
            attempt_recovery(
                writer,
                reader,
                db,
                &attempt_id,
                candidate,
                settings,
                first_venue,
                &first_mkt_id,
                first_sel,
                s1,
                o1,
                &fill1,
                Some(&second_mkt_id),
                Some(second_sel),
                reason,
            )
            .await;
            return None;
        }
    } else {
        let post_fill_sigma = 1.0 / o1 + 1.0 / second_odds;
        if post_fill_sigma >= 1.0 {
            let reason = format!("post_fill_sigma_dead sigma={:.4}", post_fill_sigma);
            tracing::warn!(
                "[POST-FILL SIGMA] Arb closed after leg 1 fill: sigma={:.4} ({}@{:.3}, {}@{:.3})",
                post_fill_sigma,
                first_sel,
                o1,
                second_sel,
                second_odds
            );
            attempt_recovery(
                writer,
                reader,
                db,
                &attempt_id,
                candidate,
                settings,
                first_venue,
                &first_mkt_id,
                first_sel,
                s1,
                o1,
                &fill1,
                None,
                None,
                &reason,
            )
            .await;
            return None;
        }
    }

    tracing::info!(
        "LEG 2: {} ${:.2} on {} @ {:.3}",
        second_venue,
        second_stake,
        second_sel,
        second_odds
    );
    emit_attempt(
        "LEG2_SENT",
        &attempt_id,
        json!({
            "venue": second_venue,
            "venue_market_id": second_mkt_id,
            "selection": second_sel,
            "stake": second_stake,
            "odds": second_odds,
        }),
    );

    let fill2 = match place_leg(
        writer,
        reader,
        &format!("{}-L2", attempt_id),
        second_venue,
        &second_mkt_id,
        second_sel,
        second_stake,
        second_odds,
    )
    .await
    {
        Ok(f) if f.status == "filled" => f,
        Ok(f) => {
            let status = f.status.clone();
            let err = f.error_message.clone().unwrap_or_default();
            tracing::error!(
                "LEG 2 FAILED: {} - {}. WARNING: LEG 1 PLACED, EXPOSED!",
                status,
                err
            );
            drain_balance_updates(reader, db).await;
            emit_attempt(
                "LEG2_FAILED",
                &attempt_id,
                json!({ "status": status, "error": err }),
            );
            attempt_recovery(
                writer,
                reader,
                db,
                &attempt_id,
                candidate,
                settings,
                first_venue,
                &first_mkt_id,
                first_sel,
                s1,
                o1,
                &fill1,
                if second_venue == "pinnacle" {
                    Some(&second_mkt_id)
                } else {
                    None
                },
                if second_venue == "pinnacle" {
                    Some(second_sel)
                } else {
                    None
                },
                &err,
            )
            .await;
            return None;
        }
        Err(e) => {
            tracing::error!("LEG 2 ERROR: {}. WARNING: LEG 1 PLACED, EXPOSED!", e);
            emit_attempt("LEG2_ERROR", &attempt_id, json!({ "error": e }));
            attempt_recovery(
                writer,
                reader,
                db,
                &attempt_id,
                candidate,
                settings,
                first_venue,
                &first_mkt_id,
                first_sel,
                s1,
                o1,
                &fill1,
                if second_venue == "pinnacle" {
                    Some(&second_mkt_id)
                } else {
                    None
                },
                if second_venue == "pinnacle" {
                    Some(second_sel)
                } else {
                    None
                },
                &e,
            )
            .await;
            return None;
        }
    };

    drain_balance_updates(reader, db).await;

    let s2 = fill2.accepted_stake.unwrap_or(second_stake);
    let o2 = fill2.accepted_odds.unwrap_or(second_odds);

    let payout1 = s1 * o1;
    let payout2 = s2 * o2;
    let cost = s1 + s2;
    let profit = payout1.min(payout2) - cost;

    let now = chrono::Utc::now().to_rfc3339();
    let trade = PaperTrade {
        id: attempt_id.clone(),
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

    emit_attempt(
        "BOTH_FILLED",
        &attempt_id,
        json!({
            "profit": profit,
            "leg1_stake": s1,
            "leg1_odds": o1,
            "leg2_stake": s2,
            "leg2_odds": o2,
        }),
    );
    tracing::info!(
        "BOTH LEGS FILLED! profit=${:.2} | {}@{:.3} ${:.2} + {}@{:.3} ${:.2}",
        profit,
        first_sel,
        o1,
        s1,
        second_sel,
        o2,
        s2
    );

    Some(trade)
}

#[cfg(test)]
mod tests {
    use super::build_pin_rescue_market_ids;

    #[test]
    fn rescue_market_ids_expand_totals() {
        let ids = build_pin_rescue_market_ids("1627197304:s;0;ou;6.5:under");
        assert_eq!(ids[0], "1627197304:s;0;ou;6.5:under");
        assert!(ids.contains(&"1627197304:s;0;ou;7.0:under".to_string()));
        assert!(ids.contains(&"1627197304:s;0;ou;4.5:under".to_string()));
    }

    #[test]
    fn rescue_market_ids_expand_spreads() {
        let ids = build_pin_rescue_market_ids("1627197304:s;0;s;-3.5:home");
        assert!(ids.contains(&"1627197304:s;0;s;-3.0:home".to_string()));
        assert!(ids.contains(&"1627197304:s;0;s;-5.0:home".to_string()));
    }

    #[test]
    fn rescue_market_ids_keep_moneyline_to_original() {
        let ids = build_pin_rescue_market_ids("1627197304:s;0;m:home");
        assert_eq!(ids, vec!["1627197304:s;0;m:home".to_string()]);
    }
}
