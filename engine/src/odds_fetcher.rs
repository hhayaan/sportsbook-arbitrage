use crate::config;
use crate::models::CanonicalMarket;
use crate::team_names;
use reqwest::Client;
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64
}

/// Detect market period from label text (DK/FD market names).
fn detect_period(label: &str) -> &'static str {
    let lower = label.to_lowercase();
    if lower.contains("1st half") || lower.contains("first half") {
        "first_half"
    } else if lower.contains("2nd half") || lower.contains("second half") {
        "second_half"
    } else if lower.contains("1st quarter") || lower.contains("1q") {
        "first_quarter"
    } else if lower.contains("2nd quarter") || lower.contains("2q") {
        "second_quarter"
    } else if lower.contains("3rd quarter") || lower.contains("3q") {
        "third_quarter"
    } else if lower.contains("4th quarter") || lower.contains("4q") {
        "fourth_quarter"
    } else if lower.contains("1st period") {
        "first_period"
    } else if lower.contains("2nd period") {
        "second_period"
    } else if lower.contains("3rd period") {
        "third_period"
    } else {
        "full_game"
    }
}

fn american_to_decimal(american: f64) -> f64 {
    if american > 0.0 {
        1.0 + american / 100.0
    } else {
        1.0 + 100.0 / american.abs()
    }
}

pub async fn fetch_all(client: &Client) -> Vec<CanonicalMarket> {
    let sports = config::sports();
    let mut handles: Vec<tokio::task::JoinHandle<Vec<CanonicalMarket>>> = Vec::new();

    for sport in &sports {
        // DraftKings — spawn concurrent
        if let Some(dk_id) = sport.dk_id {
            let c = client.clone();
            let name = sport.name;
            handles.push(tokio::spawn(async move {
                match fetch_draftkings(&c, name, dk_id).await {
                    Ok(markets) => {
                        tracing::info!("[DK]    {:<10} → {} markets", name, markets.len());
                        markets
                    }
                    Err(e) => {
                        tracing::warn!("[DK] {} fetch failed: {}", name, e);
                        Vec::new()
                    }
                }
            }));
        }

        // FanDuel — spawn concurrent
        if let Some(page) = sport.fd_page {
            let c = client.clone();
            let name = sport.name;
            handles.push(tokio::spawn(async move {
                match fetch_fanduel(&c, name, page).await {
                    Ok(markets) => {
                        tracing::info!("[FD]    {:<10} → {} markets", name, markets.len());
                        markets
                    }
                    Err(e) => {
                        tracing::warn!("[FD] {} fetch failed: {}", name, e);
                        Vec::new()
                    }
                }
            }));
        } else if let Some(event_type_id) = sport.fd_event_type {
            let c = client.clone();
            let name = sport.name;
            handles.push(tokio::spawn(async move {
                match fetch_fanduel_by_event_type(&c, name, event_type_id).await {
                    Ok(markets) => {
                        tracing::info!("[FD-ET] {:<10} → {} markets", name, markets.len());
                        markets
                    }
                    Err(e) => {
                        tracing::warn!("[FD-ET] {} fetch failed: {}", name, e);
                        Vec::new()
                    }
                }
            }));
        }

        // Pinnacle — spawn concurrent
        for &league_id in &sport.pin_league_ids {
            let c = client.clone();
            let name = sport.name;
            handles.push(tokio::spawn(async move {
                match fetch_pinnacle(&c, name, league_id).await {
                    Ok(markets) => {
                        tracing::info!("[PIN]   {:<10} → {} markets", name, markets.len());
                        markets
                    }
                    Err(e) => {
                        tracing::warn!("[PIN] {} fetch failed: {}", name, e);
                        Vec::new()
                    }
                }
            }));
        }
    }

    // Collect all results concurrently
    let mut all = Vec::new();
    for handle in handles {
        if let Ok(markets) = handle.await {
            all.extend(markets);
        }
    }
    all
}

// ── DraftKings ──────────────────────────────────────────────────────────────

async fn fetch_draftkings(
    client: &Client,
    sport: &str,
    group_id: u64,
) -> Result<Vec<CanonicalMarket>, String> {
    // Use local Node.js proxy (dk-proxy.js on port 3403) to work around
    // reqwest TLS incompatibility with DK's CDN on Windows.
    let proxy_url = format!("http://localhost:3403/dk/{}", group_id);

    match fetch_dk_inner(client, &proxy_url, sport, group_id).await {
        Ok(markets) if !markets.is_empty() => return Ok(markets),
        Ok(_) => {}
        Err(e) => {
            tracing::debug!("DK proxy failed: {}, trying direct...", e);
        }
    }

    // Fallback: try direct (works on Linux/AWS, fails on Windows)
    let direct_url = format!(
        "https://{}/sites/{}/api/sportscontent/dkcaon/v1/leagues/{}",
        config::DK_HOSTNAME, config::DK_SITE_CODE, group_id
    );
    fetch_dk_inner(client, &direct_url, sport, group_id).await
}

async fn fetch_dk_inner(
    client: &Client,
    url: &str,
    sport: &str,
    _group_id: u64,
) -> Result<Vec<CanonicalMarket>, String> {

    let resp = client
        .get(url)
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .header("Accept", "application/json")
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| format!("request failed: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("status {}", resp.status()));
    }

    let json: Value = resp.json().await.map_err(|e| format!("json parse: {}", e))?;
    let ts = now_ms();
    let mut out = Vec::new();

    // ── Sportscontent API format: events[], markets[], selections[] ──
    let events = json.get("events").and_then(|v| v.as_array());
    let dk_markets = json.get("markets").and_then(|v| v.as_array());
    let selections = json.get("selections").and_then(|v| v.as_array());

    let (events, dk_markets, selections) = match (events, dk_markets, selections) {
        (Some(e), Some(m), Some(s)) => (e, m, s),
        _ => return Ok(out),
    };

    // Helper: extract string from Value (handles both string and number types)
    fn val_str(v: &Value) -> String {
        match v {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            _ => v.to_string().replace('"', ""),
        }
    }

    // Build event lookup (string keys — DK returns IDs as strings)
    let mut events_map: std::collections::HashMap<String, &Value> = std::collections::HashMap::new();
    for ev in events {
        if let Some(id) = ev.get("id") {
            events_map.insert(val_str(id), ev);
        }
    }

    // Build market lookup by eventId, filter for main markets we care about
    let mut markets_by_event: std::collections::HashMap<String, Vec<&Value>> = std::collections::HashMap::new();
    for mkt in dk_markets {
        let is_main = mkt.get("main").and_then(|v| v.as_bool()).unwrap_or(false);
        if !is_main { continue; }
        let name = mkt.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let mkt_type = resolve_dk_market_type(name);
        if mkt_type.is_empty() { continue; }
        if let Some(eid) = mkt.get("eventId") {
            markets_by_event.entry(val_str(eid)).or_default().push(mkt);
        }
    }

    // Build selection lookup by marketId
    let mut sels_by_market: std::collections::HashMap<String, Vec<&Value>> = std::collections::HashMap::new();
    for sel in selections {
        if let Some(mid) = sel.get("marketId") {
            sels_by_market.entry(val_str(mid)).or_default().push(sel);
        }
    }

    for (eid_str, event) in &events_map {
        let event_name = event.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let start_time = event.get("startEventDate").and_then(|v| v.as_str()).unwrap_or("");
        let is_live = false; // Sportscontent API is pre-match

        let participants = event.get("participants").and_then(|v| v.as_array());
        let (dk_away, dk_home) = if let Some(parts) = participants {
            let away = parts.iter().find(|p| p.get("venueRole").and_then(|v| v.as_str()) == Some("Away"))
                .or_else(|| parts.first());
            let home = parts.iter().find(|p| p.get("venueRole").and_then(|v| v.as_str()) == Some("Home"))
                .or_else(|| parts.get(1));
            let away_name = away.and_then(|p| p.get("name").and_then(|v| v.as_str())).unwrap_or("");
            let home_name = home.and_then(|p| p.get("name").and_then(|v| v.as_str())).unwrap_or("");
            (away_name.to_string(), home_name.to_string())
        } else {
            parse_dk_event_teams(event_name)
        };

        let dk_event_id = team_names::canonical_event_id(sport, &dk_away, &dk_home, start_time);

        for mkt in markets_by_event.get(eid_str).unwrap_or(&Vec::new()) {
            let mkt_name = mkt.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let market_type = resolve_dk_market_type(mkt_name);
            let mkt_id = mkt.get("id").map(|v| val_str(v)).unwrap_or_default();

            for sel in sels_by_market.get(&mkt_id).unwrap_or(&Vec::new()) {
                let label = sel.get("label").and_then(|v| v.as_str()).unwrap_or("");
                let sel_id = sel.get("id").and_then(|v| v.as_str()).unwrap_or(""); // e.g., "0ML84048103_3"

                let dec = sel.get("displayOdds")
                    .and_then(|d| d.get("decimal"))
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse::<f64>().ok())
                    .or_else(|| sel.get("trueOdds").and_then(|v| v.as_f64()));
                let american = sel.get("displayOdds")
                    .and_then(|d| d.get("american"))
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.replace('−', "-").replace('+', "").parse::<f64>().ok());

                let decimal = match dec {
                    Some(d) if d > 1.0 && d < 100.0 => d,
                    _ => continue,
                };

                let line = sel.get("points").and_then(|v| v.as_f64());
                let sel_normalized = team_names::normalize_selection(&market_type, label, sport);

                out.push(CanonicalMarket {
                    event_id: dk_event_id.clone(),
                    sport: sport.to_string(),
                    league: sport.to_string(),
                    start_time_utc: start_time.to_string(),
                    market_type: market_type.clone(),
                    selection: sel_normalized,
                    line,
                    period: detect_period(mkt_name).to_string(),
                    is_live,
                    player_name: None,
                    prop_type: None,
                    decimal_odds: decimal,
                    american_odds: american,
                    venue: "draftkings".to_string(),
                    venue_market_id: sel_id.to_string(),
                    venue_event_id: eid_str.replace('"', ""),
                    received_at_ms: ts,
                });
            }
        }
    }

    Ok(out)
}

fn parse_dk_offer(
    offer: &Value,
    events_map: &std::collections::HashMap<u64, &Value>,
    sport: &str,
    ts: u64,
    out: &mut Vec<CanonicalMarket>,
) {
    let event_id_num = match offer.get("eventId").and_then(|v| v.as_u64()) {
        Some(id) => id,
        None => return,
    };
    let event = match events_map.get(&event_id_num) {
        Some(ev) => *ev,
        None => return,
    };

    let event_name = event.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let start_time = event.get("startDate").and_then(|v| v.as_str()).unwrap_or("");
    let is_live = event.get("eventStatus")
        .and_then(|v| v.get("state"))
        .and_then(|v| v.as_str())
        .map(|s| s == "STARTED")
        .unwrap_or(false);

    let label = offer.get("label").and_then(|v| v.as_str()).unwrap_or("");
    let market_type = resolve_dk_market_type(label);
    if market_type.is_empty() {
        return;
    }

    // Parse home/away from event name (DK format: "Away at/vs Home" or "Away @ Home")
    let (dk_away, dk_home) = parse_dk_event_teams(event_name);
    let dk_event_id = team_names::canonical_event_id(sport, &dk_away, &dk_home, start_time);
    let offer_id = offer.get("providerOfferId").and_then(|v| v.as_str()).unwrap_or("");

    if let Some(outcomes) = offer.get("outcomes").and_then(|v| v.as_array()) {
        for outcome in outcomes {
            let selection = outcome.get("label").and_then(|v| v.as_str()).unwrap_or("");
            let odds_decimal = outcome
                .get("oddsDecimal")
                .and_then(|v| v.as_f64())
                .or_else(|| {
                    outcome.get("odds").and_then(|v| v.as_f64())
                });
            let odds_american = outcome.get("oddsAmerican").and_then(|v| v.as_str())
                .and_then(|s| s.replace('+', "").parse::<f64>().ok());

            let decimal = match odds_decimal {
                Some(d) if d > 1.0 => d,
                _ => match odds_american {
                    Some(a) => american_to_decimal(a),
                    None => continue,
                },
            };

            if decimal <= 1.0 || decimal > 100.0 {
                continue;
            }

            let line = outcome.get("line").and_then(|v| v.as_f64())
                .or_else(|| outcome.get("points").and_then(|v| v.as_f64()));

            let sel_normalized = team_names::normalize_selection(&market_type, selection, sport);

            out.push(CanonicalMarket {
                event_id: dk_event_id.clone(),
                sport: sport.to_string(),
                league: sport.to_string(),
                start_time_utc: start_time.to_string(),
                market_type: market_type.clone(),
                selection: sel_normalized,
                line,
                period: detect_period(label).to_string(),
                is_live,
                player_name: None,
                prop_type: None,
                decimal_odds: decimal,
                american_odds: odds_american,
                venue: "draftkings".to_string(),
                venue_market_id: format!("dk:{}:{}", offer_id, selection),
                venue_event_id: format!("dk:{}", event_id_num),
                received_at_ms: ts,
            });
        }
    }
}

/// Parse DK event name like "Golden State Warriors at Boston Celtics" → (away, home)
fn parse_dk_event_teams(event_name: &str) -> (String, String) {
    // DK uses " at ", " vs ", " @ " as separators
    for sep in [" at ", " vs ", " @ ", " vs. "] {
        if let Some(pos) = event_name.to_lowercase().find(sep) {
            let away = event_name[..pos].trim().to_string();
            let home = event_name[pos + sep.len()..].trim().to_string();
            return (away, home);
        }
    }
    // Fallback: can't parse, use whole name
    (event_name.to_string(), "unknown".to_string())
}

fn resolve_dk_market_type(label: &str) -> String {
    let lower = label.to_lowercase();
    if lower.contains("moneyline") || lower == "money line" {
        "moneyline".to_string()
    } else if lower.contains("spread") || lower.contains("point spread") {
        "spread".to_string()
    } else if lower.contains("total") && lower.contains("team") {
        "team_total".to_string()
    } else if lower.contains("total") || lower.contains("over/under") || lower.contains("o/u") {
        "total".to_string()
    } else {
        String::new()
    }
}

// ── FanDuel ─────────────────────────────────────────────────────────────────

async fn fetch_fanduel(
    client: &Client,
    sport: &str,
    page: &str,
) -> Result<Vec<CanonicalMarket>, String> {
    // Try multiple states with CUSTOM page type
    for state in config::FD_STATES {
        let url = format!(
            "https://sbapi.{}.sportsbook.fanduel.ca/api/content-managed-page?page=CUSTOM&customPageId={}&_ak={}",
            state, page, config::FD_API_KEY
        );

        let resp = match client
            .get(&url)
            .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
            .header("Accept", "application/json")
            .timeout(std::time::Duration::from_secs(15))
            .send()
            .await
        {
            Ok(r) if r.status().is_success() => r,
            Ok(r) => {
                tracing::debug!("[FD] {} state={} status={}", sport, state, r.status());
                continue;
            }
            Err(e) => {
                tracing::debug!("[FD] {} state={} err={}", sport, state, e);
                continue;
            }
        };

        match resp.json::<Value>().await {
            Ok(json) if json.get("attachments").is_some() => {
                return parse_fd_response(&json, sport);
            }
            Ok(_) => continue,
            Err(e) => {
                tracing::debug!("[FD] {} state={} parse err={}", sport, state, e);
                continue;
            }
        }
    }
    Err(format!("FD {} all states failed", sport))
}

/// Fetch FanDuel soccer/international sports via eventTypeId endpoint.
async fn fetch_fanduel_by_event_type(
    client: &Client,
    sport: &str,
    event_type_id: u64,
) -> Result<Vec<CanonicalMarket>, String> {
    for state in config::FD_STATES {
        let url = format!(
            "https://sbapi.{}.sportsbook.fanduel.ca/api/content-managed-page?page=SPORT&eventTypeId={}&_ak={}",
            state, event_type_id, config::FD_API_KEY
        );

        let resp = match client
            .get(&url)
            .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
            .header("Accept", "application/json")
            .timeout(std::time::Duration::from_secs(15))
            .send()
            .await
        {
            Ok(r) if r.status().is_success() => r,
            Ok(r) => {
                tracing::debug!("[FD-ET] {} state={} status={}", sport, state, r.status());
                continue;
            }
            Err(e) => {
                tracing::debug!("[FD-ET] {} state={} err={}", sport, state, e);
                continue;
            }
        };

        match resp.json::<Value>().await {
            Ok(json) if json.get("attachments").is_some() => {
                return parse_fd_response(&json, sport);
            }
            Ok(_) => continue,
            Err(e) => {
                tracing::debug!("[FD-ET] {} state={} parse err={}", sport, state, e);
                continue;
            }
        }
    }
    Err(format!("FD-ET {} all states failed", sport))
}

fn parse_fd_response(json: &Value, sport: &str) -> Result<Vec<CanonicalMarket>, String> {
    let ts = now_ms();
    let mut markets = Vec::new();

    let attachments = match json.get("attachments") {
        Some(a) => a,
        None => return Ok(markets),
    };

    // Build event lookup
    let mut events_map: std::collections::HashMap<String, &Value> = std::collections::HashMap::new();
    if let Some(events) = attachments.get("events").and_then(|v| v.as_object()) {
        for (k, v) in events {
            events_map.insert(k.clone(), v);
        }
    }

    // Parse markets
    if let Some(fd_markets) = attachments.get("markets").and_then(|v| v.as_object()) {
        for (_market_id, market) in fd_markets {
            let event_id_str = match market.get("eventId") {
                Some(v) if v.is_string() => v.as_str().unwrap().to_string(),
                Some(v) if v.is_number() => {
                    // FD eventId can be a number — convert to string for map lookup
                    if let Some(n) = v.as_u64() {
                        n.to_string()
                    } else if let Some(n) = v.as_i64() {
                        n.to_string()
                    } else {
                        continue;
                    }
                }
                _ => continue,
            };

            let event = events_map.get(&event_id_str);
            let event_name = event
                .and_then(|e| e.get("name").and_then(|v| v.as_str()))
                .unwrap_or("unknown");
            let start_time = event
                .and_then(|e| e.get("openDate").and_then(|v| v.as_str()))
                .unwrap_or("");
            let is_live = event
                .and_then(|e| e.get("inPlay").and_then(|v| v.as_bool()))
                .unwrap_or(false);

            let market_name = market.get("marketName").and_then(|v| v.as_str()).unwrap_or("");
            let market_type_raw = market.get("marketType").and_then(|v| v.as_str()).unwrap_or(market_name);
            let market_type = resolve_fd_market_type(market_type_raw, market_name);
            if market_type.is_empty() {
                continue;
            }

            // Parse FD event name: "Team A @ Team B" or "Team A v Team B"
            let (fd_away, fd_home) = parse_fd_event_teams(event_name);
            // Skip futures/props/awards that don't parse into two teams
            if fd_home == "unknown" || fd_away == "unknown" {
                continue;
            }
            let fd_event_id = team_names::canonical_event_id(sport, &fd_away, &fd_home, start_time);

            if let Some(runners) = market.get("runners").and_then(|v| v.as_array()) {
                for runner in runners {
                    let runner_name = runner.get("runnerName").and_then(|v| v.as_str()).unwrap_or("");
                    let handicap = runner.get("handicap").and_then(|v| v.as_f64());

                    let decimal = runner
                        .get("winRunnerOdds")
                        .and_then(|wo| wo.get("trueOdds"))
                        .and_then(|to| to.get("decimalOdds"))
                        .and_then(|d| d.get("decimalOdds"))
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0);

                    let american = runner
                        .get("winRunnerOdds")
                        .and_then(|wo| wo.get("americanDisplayOdds"))
                        .and_then(|ao| ao.get("americanOdds"))
                        .and_then(|v| v.as_f64());

                    if decimal <= 1.0 || decimal > 100.0 {
                        continue;
                    }

                    let sel_normalized = team_names::normalize_selection(&market_type, runner_name, sport);

                    let period = detect_period(market_name);

                    // Build adapter-ready selection ID: "marketId:selectionId"
                    let market_id_str = market.get("marketId")
                        .and_then(|v| if v.is_string() { v.as_str().map(|s| s.to_string()) } else { v.as_u64().map(|n| n.to_string()) })
                        .unwrap_or_default();
                    let selection_id_str = runner.get("selectionId")
                        .and_then(|v| if v.is_string() { v.as_str().map(|s| s.to_string()) } else { v.as_u64().map(|n| n.to_string()) })
                        .unwrap_or_default();
                    let adapter_sel_id = format!("{}:{}", market_id_str, selection_id_str);

                    out_push_fd(
                        &mut markets,
                        &fd_event_id, sport, start_time, &market_type,
                        &sel_normalized, handicap, decimal, american,
                        is_live, &adapter_sel_id, runner_name, ts, period,
                    );
                }
            }
        }
    }

    Ok(markets)
}

fn out_push_fd(
    out: &mut Vec<CanonicalMarket>,
    event_id: &str, sport: &str, start_time: &str, market_type: &str,
    selection: &str, line: Option<f64>, decimal: f64, american: Option<f64>,
    is_live: bool, adapter_sel_id: &str, _runner_name: &str, ts: u64, period: &str,
) {
    out.push(CanonicalMarket {
        event_id: event_id.to_string(),
        sport: sport.to_string(),
        league: sport.to_string(),
        start_time_utc: start_time.to_string(),
        market_type: market_type.to_string(),
        selection: selection.to_string(),
        line,
        period: period.to_string(),
        is_live,
        player_name: None,
        prop_type: None,
        decimal_odds: decimal,
        american_odds: american,
        venue: "fanduel".to_string(),
        venue_market_id: adapter_sel_id.to_string(),
        venue_event_id: adapter_sel_id.to_string(),
        received_at_ms: ts,
    });
}

/// Parse FD event name like "Celtics v Warriors" or "Boston Celtics vs Golden State Warriors"
fn parse_fd_event_teams(event_name: &str) -> (String, String) {
    for sep in [" v ", " vs ", " vs. ", " @ ", " at "] {
        if let Some(pos) = event_name.to_lowercase().find(sep) {
            let away = event_name[..pos].trim().to_string();
            let home = event_name[pos + sep.len()..].trim().to_string();
            return (away, home);
        }
    }
    (event_name.to_string(), "unknown".to_string())
}

fn resolve_fd_market_type(market_type: &str, market_name: &str) -> String {
    let lower_type = market_type.to_lowercase();
    let lower_name = market_name.to_lowercase();
    if lower_type.contains("moneyline") || lower_name.contains("moneyline")
        || lower_type.contains("match_betting") || lower_type.contains("match betting")
    {
        "moneyline".to_string()
    } else if lower_type.contains("spread") || lower_name.contains("spread")
        || lower_type.contains("handicap")
    {
        "spread".to_string()
    } else if lower_type.contains("team_total") || lower_name.contains("team total") {
        "team_total".to_string()
    } else if lower_type.contains("total") || lower_name.contains("total")
        || lower_name.contains("over/under")
    {
        "total".to_string()
    } else {
        String::new()
    }
}

// ── Pinnacle ────────────────────────────────────────────────────────────────

async fn fetch_pinnacle(
    client: &Client,
    sport: &str,
    league_id: u64,
) -> Result<Vec<CanonicalMarket>, String> {
    // Step 1: Fetch matchups (events)
    let matchups_url = format!(
        "https://{}/0.1/leagues/{}/matchups",
        config::PIN_DOMAIN, league_id
    );
    let matchups_resp = client
        .get(&matchups_url)
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !matchups_resp.status().is_success() {
        return Err(format!("PIN matchups {} status: {}", sport, matchups_resp.status()));
    }

    let matchups_json: Value = matchups_resp.json().await.map_err(|e| e.to_string())?;

    // Step 2: Fetch markets (odds)
    let markets_url = format!(
        "https://{}/0.1/leagues/{}/markets/straight",
        config::PIN_DOMAIN, league_id
    );
    let markets_resp = client
        .get(&markets_url)
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !markets_resp.status().is_success() {
        return Err(format!("PIN markets {} status: {}", sport, markets_resp.status()));
    }

    let markets_json: Value = markets_resp.json().await.map_err(|e| e.to_string())?;
    let ts = now_ms();

    // Build matchup lookup: id → {participants, startTime, isLive}
    let mut matchup_map: std::collections::HashMap<u64, MatchupInfo> = std::collections::HashMap::new();
    if let Some(arr) = matchups_json.as_array() {
        for m in arr {
            let m_type = m.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if m_type != "matchup" {
                continue;
            }
            let id = match m.get("id").and_then(|v| v.as_u64()) {
                Some(id) => id,
                None => continue,
            };
            let start_time = m.get("startTime").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let is_live = m.get("isLive").and_then(|v| v.as_bool()).unwrap_or(false);

            let mut home = String::new();
            let mut away = String::new();
            let mut home_id: Option<u64> = None;
            let mut away_id: Option<u64> = None;

            if let Some(parts) = m.get("participants").and_then(|v| v.as_array()) {
                for p in parts {
                    let align = p.get("alignment").and_then(|v| v.as_str()).unwrap_or("");
                    let name = p.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    let pid = p.get("id").and_then(|v| v.as_u64());
                    if align == "home" {
                        home = name.to_string();
                        home_id = pid;
                    } else {
                        away = name.to_string();
                        away_id = pid;
                    }
                }
            }

            matchup_map.insert(id, MatchupInfo { home, away, home_id, away_id, start_time, is_live });
        }
    }

    let mut out = Vec::new();

    // Parse straight markets
    if let Some(arr) = markets_json.as_array() {
        for mkt in arr {
            let matchup_id = match mkt.get("matchupId").and_then(|v| v.as_u64()) {
                Some(id) => id,
                None => continue,
            };
            let info = match matchup_map.get(&matchup_id) {
                Some(i) => i,
                None => continue,
            };

            // Skip alternate lines — only use main markets
            let is_alternate = mkt.get("isAlternate").and_then(|v| v.as_bool()).unwrap_or(false);
            if is_alternate { continue; }

            let mtype = mkt.get("type").and_then(|v| v.as_str()).unwrap_or("");
            let market_type = match mtype {
                "moneyline" => "moneyline",
                "spread" => "spread",
                "total" => "total",
                "team_total" => "team_total",
                _ => continue,
            };

            let period = mkt.get("period").and_then(|v| v.as_u64()).unwrap_or(0);
            let period_str = match period {
                0 => "full_game",
                1 => "first_half",
                2 => "second_half",
                _ => "full_game",
            };

            let pin_event_id = team_names::canonical_event_id(sport, &info.away, &info.home, &info.start_time);

            if let Some(prices) = mkt.get("prices").and_then(|v| v.as_array()) {
                for price in prices {
                    let american_odds = price.get("price").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    if american_odds == 0.0 {
                        continue;
                    }
                    let decimal = american_to_decimal(american_odds);
                    if decimal <= 1.0 || decimal > 100.0 {
                        continue;
                    }

                    let designation = price.get("designation").and_then(|v| v.as_str()).unwrap_or("");
                    let points = price.get("points").and_then(|v| v.as_f64());
                    let participant_id = price.get("participantId").and_then(|v| v.as_u64());

                    let raw_selection = resolve_pin_selection(
                        market_type, designation, participant_id, &info,
                    );
                    if raw_selection.is_empty() {
                        continue;
                    }
                    let selection = team_names::normalize_selection(market_type, &raw_selection, sport);

                    out.push(CanonicalMarket {
                        event_id: pin_event_id.clone(),
                        sport: sport.to_string(),
                        league: sport.to_string(),
                        start_time_utc: info.start_time.clone(),
                        market_type: market_type.to_string(),
                        selection,
                        line: points,
                        period: period_str.to_string(),
                        is_live: info.is_live,
                        player_name: None,
                        prop_type: None,
                        decimal_odds: decimal,
                        american_odds: Some(american_odds),
                        venue: "pinnacle".to_string(),
                        venue_market_id: {
                            // Adapter-ready: "{matchupId}:{marketKey}:{designation}"
                            let mkey = match market_type {
                                "moneyline" => format!("s;{};m", period),
                                "spread" => format!("s;{};s;{}", period, points.map(|p| format!("{:.1}", p)).unwrap_or_default()),
                                "total" => format!("s;{};ou;{}", period, points.map(|p| format!("{:.1}", p)).unwrap_or_default()),
                                "team_total" => format!("s;{};tt;{};{}", period, points.map(|p| format!("{:.1}", p)).unwrap_or_default(), designation),
                                _ => format!("s;{};m", period),
                            };
                            format!("{}:{}:{}", matchup_id, mkey, designation)
                        },
                        venue_event_id: format!("pin:{}", matchup_id),
                        received_at_ms: ts,
                    });
                }
            }
        }
    }

    Ok(out)
}

struct MatchupInfo {
    home: String,
    away: String,
    home_id: Option<u64>,
    away_id: Option<u64>,
    start_time: String,
    is_live: bool,
}

fn resolve_pin_selection(
    market_type: &str,
    designation: &str,
    participant_id: Option<u64>,
    info: &MatchupInfo,
) -> String {
    match market_type {
        "moneyline" | "spread" => {
            // Resolve by participant ID or designation
            if let Some(pid) = participant_id {
                if info.home_id == Some(pid) {
                    return info.home.clone();
                }
                if info.away_id == Some(pid) {
                    return info.away.clone();
                }
            }
            match designation {
                "home" => info.home.clone(),
                "away" => info.away.clone(),
                _ => String::new(),
            }
        }
        "total" | "team_total" => {
            match designation {
                "over" => "over".to_string(),
                "under" => "under".to_string(),
                _ => String::new(),
            }
        }
        _ => String::new(),
    }
}

// Selection normalization moved to team_names::normalize_selection()
