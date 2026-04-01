use axum::{
    extract::{State, ws::WebSocketUpgrade},
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use std::sync::Arc;
use crate::db::Db;
use crate::models::*;
use crate::ws;

pub struct AppState {
    pub db: Arc<Db>,
    pub ws_tx: Arc<ws::WsTx>,
}

// ── Balances ────────────────────────────────────────────────────────────────

pub async fn get_balances(State(state): State<Arc<AppState>>) -> Json<Vec<BookBalance>> {
    Json(state.db.get_balances())
}

#[derive(Deserialize)]
pub struct BalanceAction {
    pub venue: String,
    pub amount: f64,
}

pub async fn deposit(
    State(state): State<Arc<AppState>>,
    Json(body): Json<BalanceAction>,
) -> Json<Vec<BookBalance>> {
    if let Some(current) = state.db.get_balance(&body.venue) {
        state.db.update_balance(&body.venue, current + body.amount);
    }
    let balances = state.db.get_balances();
    let _ = state.ws_tx.send(WsEvent::BalanceUpdated { balances: balances.clone() });
    Json(balances)
}

pub async fn withdraw(
    State(state): State<Arc<AppState>>,
    Json(body): Json<BalanceAction>,
) -> Json<Vec<BookBalance>> {
    if let Some(current) = state.db.get_balance(&body.venue) {
        let new_bal = (current - body.amount).max(0.0);
        state.db.update_balance(&body.venue, new_bal);
    }
    let balances = state.db.get_balances();
    let _ = state.ws_tx.send(WsEvent::BalanceUpdated { balances: balances.clone() });
    Json(balances)
}

pub async fn reset_balances(State(state): State<Arc<AppState>>) -> Json<Vec<BookBalance>> {
    state.db.reset_balances();
    let balances = state.db.get_balances();
    let _ = state.ws_tx.send(WsEvent::BalanceUpdated { balances: balances.clone() });
    Json(balances)
}

// ── Trades ──────────────────────────────────────────────────────────────────

pub async fn get_trades(State(state): State<Arc<AppState>>) -> Json<Vec<PaperTrade>> {
    Json(state.db.get_trades(200))
}

// ── Arbs ────────────────────────────────────────────────────────────────────

pub async fn get_arbs(State(state): State<Arc<AppState>>) -> Json<Vec<ArbOpportunity>> {
    Json(state.db.get_arbs(200))
}

// ── Stats ───────────────────────────────────────────────────────────────────

pub async fn get_stats(State(state): State<Arc<AppState>>) -> Json<Stats> {
    Json(state.db.get_stats())
}

// ── Settings ────────────────────────────────────────────────────────────────

pub async fn get_settings(State(state): State<Arc<AppState>>) -> Json<Settings> {
    Json(state.db.get_settings())
}

pub async fn update_settings(
    State(state): State<Arc<AppState>>,
    Json(settings): Json<Settings>,
) -> Json<Settings> {
    state.db.update_settings(&settings);
    let _ = state.ws_tx.send(WsEvent::SettingsUpdated { settings: settings.clone() });
    Json(settings)
}

// ── Reset ───────────────────────────────────────────────────────────────────

pub async fn full_reset(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    state.db.full_reset();
    let balances = state.db.get_balances();
    let _ = state.ws_tx.send(WsEvent::BalanceUpdated { balances });
    Json(serde_json::json!({"status": "reset complete"}))
}

// ── WebSocket ───────────────────────────────────────────────────────────────

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| ws::handle_ws(socket, state.ws_tx.clone()))
}
