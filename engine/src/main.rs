mod api;
mod arb_detector;
mod attempt_telemetry;
mod circuit_breaker;
mod config;
mod db;
mod detector_telemetry;
mod execution_math;
mod family_dedup;
mod live_executor;
mod models;
mod naked_legs;
mod odds_fetcher;
mod odds_verifier;
mod paper_executor;
mod pin_yank;
mod policy;
mod rate_limiter;
mod rescue_hedger;
mod stake_optimizer;
mod state_machine;
mod status_classifier;
mod team_names;
mod trades;
mod treasury;
mod ws;

use api::AppState;
use axum::{
    routing::{get, post},
    Router,
};
use serde_json::json;
use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;
use tower_http::cors::{Any, CorsLayer};

fn venue_alias(raw: &str) -> Option<&'static str> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "dk" | "draftkings" | "draft_kings" => Some("draftkings"),
        "fd" | "fanduel" | "fan_duel" => Some("fanduel"),
        "pin" | "pinnacle" => Some("pinnacle"),
        _ => None,
    }
}

fn canonical_venue_pair(a: &str, b: &str) -> String {
    if a <= b {
        format!("{}/{}", a, b)
    } else {
        format!("{}/{}", b, a)
    }
}

fn candidate_venue_pair_key(candidate: &models::ArbCandidate) -> String {
    canonical_venue_pair(&candidate.leg1_venue, &candidate.leg2_venue)
}

fn live_allowed_pairs_from_env() -> Option<BTreeSet<String>> {
    let raw = std::env::var("LIVE_ALLOWED_PAIRS").ok()?;
    let raw = raw.trim();
    if raw.is_empty() || raw.eq_ignore_ascii_case("all") || raw == "*" {
        return None;
    }

    let mut pairs = BTreeSet::new();
    for token in raw.split([',', ';']) {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }

        let parts: Vec<&str> = token
            .split(['/', '+', '|', '-'])
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .collect();

        if parts.len() != 2 {
            tracing::warn!(
                "Ignoring invalid LIVE_ALLOWED_PAIRS token '{}'; expected venue/venue",
                token
            );
            continue;
        }

        match (venue_alias(parts[0]), venue_alias(parts[1])) {
            (Some(a), Some(b)) if a != b => {
                pairs.insert(canonical_venue_pair(a, b));
            }
            _ => tracing::warn!(
                "Ignoring invalid LIVE_ALLOWED_PAIRS token '{}'; expected venues like fanduel/pinnacle",
                token
            ),
        }
    }

    Some(pairs)
}

fn live_allowed_pairs_label(pairs: &Option<BTreeSet<String>>) -> String {
    match pairs {
        None => "all".to_string(),
        Some(pairs) if pairs.is_empty() => "none".to_string(),
        Some(pairs) => pairs.iter().cloned().collect::<Vec<_>>().join(","),
    }
}

fn live_pair_allowed(pairs: &Option<BTreeSet<String>>, pair: &str) -> bool {
    pairs
        .as_ref()
        .map_or(true, |allowed| allowed.contains(pair))
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().with_env_filter("info").init();

    let live_mode = std::env::var("LIVE_MODE").unwrap_or_default() == "1";
    if live_mode {
        tracing::info!("=== Arb Engine V6 — LIVE TRADING ===");
    } else {
        tracing::info!("=== Arb Engine V6 — Paper Trading ===");
    }

    // Init DB
    let db = Arc::new(db::Db::open("data/trades.db"));
    tracing::info!("SQLite initialized at data/trades.db");

    // Apply env var overrides to settings
    let mut settings_overrides = db.get_settings();
    let mut any_override = false;
    if let Ok(v) = std::env::var("FIXED_STAKE") {
        if let Ok(val) = v.parse::<f64>() {
            settings_overrides.fixed_stake = val;
            any_override = true;
        }
    }
    if let Ok(v) = std::env::var("MAX_STAKE") {
        if let Ok(val) = v.parse::<f64>() {
            settings_overrides.max_stake = val;
            any_override = true;
        }
    }
    if let Ok(v) = std::env::var("MIN_ARB_PCT") {
        if let Ok(val) = v.parse::<f64>() {
            settings_overrides.min_arb_pct = val;
            any_override = true;
        }
    }
    if let Ok(v) = std::env::var("STAKE_MODE") {
        settings_overrides.stake_mode = v;
        any_override = true;
    }
    if let Ok(v) = std::env::var("PROPORTIONAL_PCT") {
        if let Ok(val) = v.parse::<f64>() {
            settings_overrides.proportional_pct = val;
            any_override = true;
        }
    }
    if let Ok(v) = std::env::var("ENABLED") {
        settings_overrides.enabled = v == "1" || v == "true";
        any_override = true;
    }
    if any_override {
        db.update_settings(&settings_overrides);
    }

    let max_hours_to_start: i64 = std::env::var("MAX_HOURS_TO_START")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(24);
    let balance_refresh_cycles: u64 = std::env::var("BALANCE_REFRESH_CYCLES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(60);
    let naked_leg_monitor_cycles: u64 = std::env::var("NAKED_LEG_MONITOR_CYCLES")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|v| *v > 0)
        .unwrap_or(10);
    let skip_live: bool = std::env::var("SKIP_LIVE")
        .map(|v| v == "1" || v == "true")
        .unwrap_or(false);
    let pin_odds_source = odds_fetcher::pinnacle_odds_source_name();
    let pin_auth_odds_base_url = odds_fetcher::pinnacle_auth_odds_base_url();
    let dk_proxy_base_url = odds_fetcher::draftkings_proxy_base_url();
    let live_allowed_pairs = live_allowed_pairs_from_env();
    let live_allowed_pairs_label = live_allowed_pairs_label(&live_allowed_pairs);

    tracing::info!(
        "Settings: enabled={} stake_mode={} fixed_stake=${:.2} max_stake=${:.2} min_arb_pct={:.1}% max_hours={} skip_live={} pin_odds_source={} live_allowed_pairs={} family_dedup_ttl={}s FAMILY_DEDUP_SUCCESS_COOLDOWN_SECS={} FAMILY_DEDUP_SUCCESS_MAX_REPEATS={}",
        settings_overrides.enabled, settings_overrides.stake_mode,
        settings_overrides.fixed_stake, settings_overrides.max_stake, settings_overrides.min_arb_pct,
        max_hours_to_start, skip_live, pin_odds_source, live_allowed_pairs_label,
        family_dedup::FamilyDedup::ttl_secs_from_env(),
        family_dedup::FamilyDedup::success_cooldown_secs_from_env(),
        family_dedup::FamilyDedup::success_max_repeats_from_env()
    );
    tracing::info!("DraftKings proxy endpoint: {}", dk_proxy_base_url);
    if pin_odds_source == "auth" {
        tracing::info!("Pinnacle auth odds bridge: {}", pin_auth_odds_base_url);
    } else {
        tracing::info!("Pinnacle odds source: guest API");
    }

    // Init WebSocket broadcaster
    let (ws_tx, _) = ws::create_broadcaster();
    let ws_tx = Arc::new(ws_tx);

    // Shared state
    let state = Arc::new(AppState {
        db: db.clone(),
        ws_tx: ws_tx.clone(),
    });

    // HTTP client for odds fetching
    let http_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .danger_accept_invalid_certs(true) // DK's TLS causes issues with native-tls on Windows
        .build()
        .expect("failed to create HTTP client");

    // Background polling task
    let poll_db = db.clone();
    let poll_ws = ws_tx.clone();
    let poll_client = http_client.clone();
    tokio::spawn(async move {
        // Connect to bet placer WebSocket if in live mode
        let ws_conn = if live_mode {
            let ws_url =
                std::env::var("WS_URL").unwrap_or_else(|_| "ws://127.0.0.1:3402".to_string());
            match live_executor::connect(&ws_url).await {
                Ok((writer, reader)) => {
                    tracing::info!("WebSocket connected to bet placer at {}", ws_url);

                    // Fetch real sportsbook balances and seed the DB
                    match live_executor::fetch_balances(&writer, &reader).await {
                        Ok(balances) => {
                            for (venue, balance) in &balances {
                                poll_db.update_balance(venue, *balance);
                                tracing::info!("[BALANCE] {:<10} = ${:.2} (live)", venue, balance);
                            }
                            let db_balances = poll_db.get_balances();
                            let _ = poll_ws.send(models::WsEvent::BalanceUpdated {
                                balances: db_balances,
                            });
                        }
                        Err(e) => {
                            tracing::warn!("Could not fetch live balances: {}", e);
                        }
                    }

                    Some((writer, reader))
                }
                Err(e) => {
                    tracing::error!("WebSocket connection failed: {} — falling back to paper", e);
                    None
                }
            }
        } else {
            None
        };
        let mut book = models::Book::new(config::MAX_DATA_AGE_MS);
        let mut family_dedup = family_dedup::FamilyDedup::load_from_env();
        tracing::info!(
            "Family dedup configured: ttl={}s success_cooldown={}s success_max_repeats={} file={}",
            family_dedup.ttl_secs(),
            family_dedup.success_cooldown_secs(),
            family_dedup.success_max_repeats(),
            std::env::var("FAMILY_DEDUP_FILE")
                .unwrap_or_else(|_| "data/family-dedup.json".to_string())
        );

        // Safety modules
        // Paper trading: generous limits (real trading would use 3-5 bets/hour)
        let mut cb = circuit_breaker::CircuitBreaker::new(500.0, 3600, 0.5, 20, 60);
        let venue_limits: std::collections::HashMap<String, u32> = config::VENUES
            .iter()
            .map(|v| (v.to_string(), 200u32)) // 200 bets/hour for paper
            .collect();
        let mut rl = rate_limiter::RateLimiter::new(venue_limits, 60);
        let mut last_venue_data: std::collections::HashMap<String, u64> =
            std::collections::HashMap::new();

        // Diff-based book updates: only update entries where odds/line/live changed
        let mut market_hashes: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        let mut diff_cycle_count: u64 = 0;

        loop {
            tracing::info!("── Polling odds from DK / FD / PIN ──");

            let markets = odds_fetcher::fetch_all(&poll_client).await;

            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64;

            // Diff-based book update: only update markets where price changed
            let mut changed = 0u64;
            let mut unchanged = 0u64;
            let mut seen_keys: std::collections::HashSet<String> = std::collections::HashSet::new();

            for m in &markets {
                let diff_key = format!("{}:{}:{}", m.venue, m.venue_market_id, m.selection);
                let hash = format!("{:.4}|{:?}|{}", m.decimal_odds, m.line, m.is_live);
                seen_keys.insert(diff_key.clone());

                let is_new = match market_hashes.get(&diff_key) {
                    Some(prev) => prev != &hash,
                    None => true,
                };

                if is_new {
                    book.update(m);
                    market_hashes.insert(diff_key, hash);
                    changed += 1;
                } else {
                    // Odds unchanged — still refresh the timestamp so expiry doesn't kill it
                    book.touch(m);
                    unchanged += 1;
                }

                last_venue_data.insert(m.venue.clone(), now_ms);
            }

            // Clean stale diff keys every 3 cycles
            diff_cycle_count += 1;
            if diff_cycle_count % 3 == 0 {
                let before = market_hashes.len();
                market_hashes.retain(|k, _| seen_keys.contains(k));
                let removed = before - market_hashes.len();
                if removed > 0 {
                    tracing::debug!("Diff cleanup: removed {} stale keys", removed);
                }
            }

            tracing::info!(
                "Fetched {} markets ({}  changed, {} unchanged)",
                markets.len(),
                changed,
                unchanged
            );

            book.expire(now_ms);
            rl.prune();

            // Log venue health
            for venue in config::VENUES {
                let age = last_venue_data
                    .get(*venue)
                    .map(|t| now_ms.saturating_sub(*t))
                    .unwrap_or(u64::MAX);
                let count = markets.iter().filter(|m| m.venue == *venue).count();
                if age > 60_000 && count == 0 {
                    tracing::warn!("[HEALTH] {} — no data for {}s", venue, age / 1000);
                }
            }

            tracing::info!(
                "Book: {} entries across {} identity keys",
                book.total_entries(),
                book.identity_keys().len(),
            );

            let detector_telemetry_enabled = detector_telemetry::enabled();
            if detector_telemetry_enabled {
                detector_telemetry::emit_market_counts(&markets);
            }

            // Detect arbs
            // DK-FD arbs lost money — require Pinnacle on one side for production
            let require_pinnacle_leg = true;
            let candidates = if detector_telemetry_enabled {
                let detection = arb_detector::analyze(
                    &book,
                    now_ms,
                    config::MAX_DATA_AGE_MS,
                    require_pinnacle_leg,
                    detector_telemetry::snapshot_limit(),
                    detector_telemetry::near_miss_sigma_max(),
                );
                detector_telemetry::emit_detection_diagnostics(&detection.diagnostics);
                detection.candidates
            } else {
                arb_detector::detect_arbs(
                    &book,
                    now_ms,
                    config::MAX_DATA_AGE_MS,
                    require_pinnacle_leg,
                )
            };

            if !candidates.is_empty() {
                tracing::info!("Detected {} arb candidates", candidates.len());
            }

            let settings = poll_db.get_settings();
            if detector_telemetry_enabled {
                detector_telemetry::emit_candidate_events(
                    &candidates,
                    settings.min_arb_pct / 100.0,
                );
            }

            for candidate in &candidates {
                let arb_id = uuid::Uuid::new_v4().to_string();
                let now_str = chrono::Utc::now().to_rfc3339();

                let arb_opp = models::ArbOpportunity {
                    id: arb_id.clone(),
                    detected_at: now_str,
                    event_id: candidate.event_id.clone(),
                    sport: candidate.sport.clone(),
                    market_type: candidate.market_type.clone(),
                    sigma: candidate.sigma,
                    arb_pct: candidate.arb_pct * 100.0,
                    leg1_venue: candidate.leg1_venue.clone(),
                    leg1_selection: candidate.leg1_selection.clone(),
                    leg1_odds: candidate.leg1_odds,
                    leg2_venue: candidate.leg2_venue.clone(),
                    leg2_selection: candidate.leg2_selection.clone(),
                    leg2_odds: candidate.leg2_odds,
                    traded: false,
                };

                poll_db.insert_arb(&arb_opp);
                let _ = poll_ws.send(models::WsEvent::ArbDetected { arb: arb_opp });

                let leg1_sel = format!("{}@{:.3}", candidate.leg1_selection, candidate.leg1_odds);
                let leg1_venue_str = format!("({})", candidate.leg1_venue);
                let leg2_sel = format!("{}@{:.3}", candidate.leg2_selection, candidate.leg2_odds);
                let leg2_venue_str = format!("({})", candidate.leg2_venue);
                let game = candidate.event_id.rsplit(':').next().unwrap_or("");
                tracing::info!(
                    "[ARB]   {:>5.2}% | {:<5} {:<10} | {:>12} {:>12} vs {:<12} {:<12} | {}",
                    candidate.arb_pct * 100.0,
                    candidate.sport,
                    candidate.market_type,
                    leg1_venue_str,
                    leg1_sel,
                    leg2_sel,
                    leg2_venue_str,
                    game,
                );

                // --- Safety checks before trading ---

                // Circuit breaker
                if let Some(reason) = cb.check() {
                    tracing::warn!("[CIRCUIT BREAKER] Trading paused: {}", reason);
                    break;
                }

                // Rate limiter
                if let Err((count, max)) = rl.check(&candidate.leg1_venue) {
                    tracing::warn!(
                        "[RATE LIMIT] {} at {}/{} bets/hour",
                        candidate.leg1_venue,
                        count,
                        max
                    );
                    continue;
                }
                if let Err((count, max)) = rl.check(&candidate.leg2_venue) {
                    tracing::warn!(
                        "[RATE LIMIT] {} at {}/{} bets/hour",
                        candidate.leg2_venue,
                        count,
                        max
                    );
                    continue;
                }

                // Odds drift verification
                if let Err(reason) = odds_verifier::verify_both_legs(&book, candidate, 0.05) {
                    tracing::info!("[ODDS DRIFT] {}", reason);
                    continue;
                }

                // High-% arb warning (warn only, don't block)
                if candidate.arb_pct > 0.15 {
                    tracing::warn!(
                        "[HIGH%] {:.1}% arb — possible mismatch: {} {}",
                        candidate.arb_pct * 100.0,
                        candidate.event_id,
                        candidate.market_type
                    );
                }

                // Skip live games if SKIP_LIVE is enabled
                if skip_live && candidate.is_live {
                    continue;
                }

                // Time-to-start filter: skip events too far in the future (capital lockup)
                if !candidate.is_live {
                    if let Ok(start) =
                        chrono::DateTime::parse_from_rfc3339(&candidate.start_time_utc)
                    {
                        let now = chrono::Utc::now();
                        let hours_to_start = (start.timestamp() - now.timestamp()).max(0) / 3600;
                        if hours_to_start > max_hours_to_start {
                            tracing::debug!(
                                "[TIME] {} {}h away, max={}h — skipping",
                                candidate.event_id,
                                hours_to_start,
                                max_hours_to_start
                            );
                            continue;
                        }
                    }
                }

                // Execute trade if enabled
                if settings.enabled {
                    // In live mode, skip arbs involving non-bettable venues
                    if live_mode {
                        let bettable = ["fanduel", "draftkings", "pinnacle"];
                        if !bettable.contains(&candidate.leg1_venue.as_str())
                            || !bettable.contains(&candidate.leg2_venue.as_str())
                        {
                            continue;
                        }

                        let live_pair = candidate_venue_pair_key(candidate);
                        if !live_pair_allowed(&live_allowed_pairs, &live_pair) {
                            tracing::info!(
                                "[LIVE PAIR SKIP] {} not in LIVE_ALLOWED_PAIRS={} | {} {} {}",
                                live_pair,
                                live_allowed_pairs_label,
                                candidate.sport,
                                candidate.market_type,
                                candidate.event_id
                            );
                            if detector_telemetry_enabled {
                                let soft_venue = if candidate.leg1_venue == "pinnacle" {
                                    candidate.leg2_venue.as_str()
                                } else {
                                    candidate.leg1_venue.as_str()
                                };
                                let _ = attempt_telemetry::emit_event(
                                    "LIVE_PAIR_SKIPPED",
                                    None,
                                    json!({
                                        "event_id": candidate.event_id.clone(),
                                        "sport": candidate.sport.clone(),
                                        "venue_pair": live_pair,
                                        "allowed_pairs": live_allowed_pairs_label.clone(),
                                        "soft_venue": soft_venue,
                                        "leg1_venue": candidate.leg1_venue.clone(),
                                        "leg2_venue": candidate.leg2_venue.clone(),
                                        "market_type": candidate.market_type.clone(),
                                        "period": candidate.period.clone(),
                                    }),
                                );
                            }
                            continue;
                        }
                    }

                    let candidate_to_trade = candidate.clone();
                    let live_execution_available = ws_conn.is_some();

                    if live_execution_available {
                        let soft_venue = if candidate_to_trade.leg1_venue == "pinnacle" {
                            candidate_to_trade.leg2_venue.as_str()
                        } else {
                            candidate_to_trade.leg1_venue.as_str()
                        };
                        let venue_pair =
                            if candidate_to_trade.leg1_venue <= candidate_to_trade.leg2_venue {
                                format!(
                                    "{}/{}",
                                    candidate_to_trade.leg1_venue, candidate_to_trade.leg2_venue
                                )
                            } else {
                                format!(
                                    "{}/{}",
                                    candidate_to_trade.leg2_venue, candidate_to_trade.leg1_venue
                                )
                            };
                        if let Some(hit) = family_dedup.lock_status(&candidate_to_trade) {
                            tracing::info!(
                                "[FAMILY DEDUP] Skip {} {} {} via {} ({}s left, policy={}, reason={}, fills={}/{}, key={})",
                                candidate_to_trade.sport,
                                candidate_to_trade.market_type,
                                candidate_to_trade.period,
                                soft_venue,
                                hit.remaining_secs,
                                hit.policy,
                                hit.reason,
                                hit.successful_fills,
                                hit.max_successful_fills,
                                hit.key
                            );
                            let _ = attempt_telemetry::emit_event(
                                "FAMILY_DEDUP_SKIP",
                                None,
                                json!({
                                    "key": hit.key,
                                    "reason": hit.reason,
                                    "policy": hit.policy,
                                    "remaining_secs": hit.remaining_secs,
                                    "successful_fills": hit.successful_fills,
                                    "max_successful_fills": hit.max_successful_fills,
                                    "event_id": candidate_to_trade.event_id,
                                    "sport": candidate_to_trade.sport,
                                    "venue_pair": venue_pair,
                                    "soft_venue": soft_venue,
                                    "leg1_venue": candidate_to_trade.leg1_venue,
                                    "leg2_venue": candidate_to_trade.leg2_venue,
                                    "market_type": candidate_to_trade.market_type,
                                    "period": candidate_to_trade.period,
                                }),
                            );
                            continue;
                        }

                        if detector_telemetry_enabled {
                            let _ = attempt_telemetry::emit_event(
                                "FAMILY_DEDUP_PASSED",
                                None,
                                json!({
                                    "event_id": candidate_to_trade.event_id.clone(),
                                    "sport": candidate_to_trade.sport.clone(),
                                    "venue_pair": venue_pair.clone(),
                                    "soft_venue": soft_venue,
                                    "leg1_venue": candidate_to_trade.leg1_venue.clone(),
                                    "leg2_venue": candidate_to_trade.leg2_venue.clone(),
                                    "market_type": candidate_to_trade.market_type.clone(),
                                    "period": candidate_to_trade.period.clone(),
                                }),
                            );
                        }
                    }

                    // Branch: live execution via WebSocket or paper execution
                    let execution_result = if let Some((ref writer, ref reader)) = ws_conn {
                        live_executor::try_execute(
                            writer,
                            reader,
                            &poll_db,
                            &candidate_to_trade,
                            &settings,
                        )
                        .await
                    } else {
                        paper_executor::try_execute(&poll_db, &candidate_to_trade, &settings)
                    };

                    if let Some(result) = execution_result {
                        if let Some(reason) = result.family_lock_reason.as_deref() {
                            if live_execution_available {
                                let entry =
                                    family_dedup.record_hard_lock(&candidate_to_trade, reason);
                                tracing::info!(
                                    "[FAMILY DEDUP] Hard-locked {} for {}s after {} (key={})",
                                    entry.event_id,
                                    family_dedup.ttl_secs(),
                                    entry.reason,
                                    entry.key
                                );
                                let _ = attempt_telemetry::emit_event(
                                    "FAMILY_DEDUP_LOCKED",
                                    None,
                                    json!({
                                        "key": entry.key,
                                        "reason": entry.reason,
                                        "event_id": entry.event_id,
                                        "sport": entry.sport,
                                        "soft_venue": entry.soft_venue,
                                        "market_type": entry.market_type,
                                        "period": entry.period,
                                        "expires_at_ms": entry.expires_at_ms,
                                        "policy": "hard_lock",
                                    }),
                                );
                            }
                        }

                        if let Some(trade) = result.trade {
                            let mode_label = if ws_conn.is_some() { "LIVE" } else { "PAPER" };
                            tracing::info!(
                                "[{}] ${:.2} profit on {} {} | {}@{:.3} + {}@{:.3}",
                                mode_label,
                                trade.theoretical_profit,
                                trade.sport,
                                trade.market_type,
                                trade.leg1_selection,
                                trade.leg1_odds,
                                trade.leg2_selection,
                                trade.leg2_odds,
                            );
                            poll_db.mark_arb_traded(&arb_id);
                            if live_execution_available {
                                let entry =
                                    family_dedup.record_successful_fill(&candidate_to_trade);
                                tracing::info!(
                                    "[FAMILY DEDUP] Recorded successful fill for {} (fills={}/{}, cooldown={}s, ttl={}s, key={})",
                                    entry.event_id,
                                    entry.successful_fills,
                                    family_dedup.max_successful_fills(),
                                    family_dedup.success_cooldown_secs(),
                                    family_dedup.ttl_secs(),
                                    entry.key
                                );
                                let _ = attempt_telemetry::emit_event(
                                    "FAMILY_DEDUP_SUCCESS_RECORDED",
                                    None,
                                    json!({
                                        "key": entry.key,
                                        "reason": entry.reason,
                                        "event_id": entry.event_id,
                                        "sport": entry.sport,
                                        "soft_venue": entry.soft_venue,
                                        "market_type": entry.market_type,
                                        "period": entry.period,
                                        "expires_at_ms": entry.expires_at_ms,
                                        "cooldown_until_ms": entry.cooldown_until_ms,
                                        "successful_fills": entry.successful_fills,
                                        "max_successful_fills": family_dedup.max_successful_fills(),
                                        "success_cooldown_secs": family_dedup.success_cooldown_secs(),
                                        "policy": "success_repeat_limited",
                                    }),
                                );
                            }

                            // Record in safety modules
                            rl.record(&candidate.leg1_venue);
                            rl.record(&candidate.leg2_venue);
                            cb.record(trade.theoretical_profit, false);

                            let balances = poll_db.get_balances();
                            let _ = poll_ws.send(models::WsEvent::TradeExecuted { trade });
                            let _ = poll_ws.send(models::WsEvent::BalanceUpdated { balances });
                        }
                    }
                }
            }

            // Clean up expired dedup entries
            family_dedup.prune_expired();

            // Broadcast poll status
            for venue in config::VENUES {
                let count = markets.iter().filter(|m| m.venue == *venue).count();
                let _ = poll_ws.send(models::WsEvent::PollStatus {
                    venue: venue.to_string(),
                    markets: count,
                });
            }

            // Periodic balance refresh (picks up winnings from settled bets)
            if diff_cycle_count % balance_refresh_cycles == 0 && diff_cycle_count > 0 {
                if let Some((ref writer, ref reader)) = ws_conn {
                    match live_executor::fetch_balances(writer, reader).await {
                        Ok(balances) => {
                            for (venue, balance) in &balances {
                                poll_db.update_balance(venue, *balance);
                            }
                            tracing::info!("[BALANCE REFRESH] synced {} venues", balances.len());
                            let db_balances = poll_db.get_balances();
                            let _ = poll_ws.send(models::WsEvent::BalanceUpdated {
                                balances: db_balances,
                            });
                        }
                        Err(e) => tracing::warn!("[BALANCE REFRESH] failed: {}", e),
                    }
                }
            }

            // Periodic naked-leg monitor: persisted exposure survives restarts and
            // gets retried for PIN relist/cashout resolution without adding a GUI.
            if diff_cycle_count % naked_leg_monitor_cycles == 0 && diff_cycle_count > 0 {
                if let Some((ref writer, ref reader)) = ws_conn {
                    let pin_balance = poll_db.get_balance("pinnacle").unwrap_or(0.0);
                    if let Err(e) =
                        live_executor::monitor_naked_legs(writer, reader, pin_balance).await
                    {
                        tracing::warn!("[NAKED LEG MONITOR] failed: {}", e);
                    }
                }
            }

            tokio::time::sleep(Duration::from_secs(config::POLL_INTERVAL_SECS)).await;
        }
    });

    // Axum router
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/api/balances", get(api::get_balances))
        .route("/api/balances/deposit", post(api::deposit))
        .route("/api/balances/withdraw", post(api::withdraw))
        .route("/api/balances/reset", post(api::reset_balances))
        .route("/api/trades", get(api::get_trades))
        .route("/api/arbs", get(api::get_arbs))
        .route("/api/stats", get(api::get_stats))
        .route(
            "/api/settings",
            get(api::get_settings).post(api::update_settings),
        )
        .route("/api/reset", post(api::full_reset))
        .route("/ws", get(api::ws_handler))
        .layer(cors)
        .with_state(state);

    let addr = "0.0.0.0:3401";
    tracing::info!("Listening on http://{}", addr);
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind");
    axum::serve(listener, app).await.expect("server failed");
}
