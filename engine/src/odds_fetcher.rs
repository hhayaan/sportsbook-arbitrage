use crate::config;
use crate::models::CanonicalMarket;
use crate::team_names;
use reqwest::Client;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const DEFAULT_ODDS_FETCH_TIMEOUT_MS: u64 = 2_500;

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

fn odds_fetch_timeout() -> Duration {
    let ms = std::env::var("ODDS_FETCH_TIMEOUT_MS")
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .filter(|ms| (250..=30_000).contains(ms))
        .unwrap_or(DEFAULT_ODDS_FETCH_TIMEOUT_MS);
    Duration::from_millis(ms)
}

/// Detect market period from label text (DK/FD market names).
fn detect_period(label: &str) -> &'static str {
    normalize_period_label(label)
}

fn normalize_period_label(label: &str) -> &'static str {
    let lower = label.to_ascii_lowercase();
    if lower.contains("00:00-09:59") || lower.contains("00:00-9:59") {
        "first_period_0_10"
    } else if lower.contains("00:00-04:59") || lower.contains("00:00-4:59") {
        "first_period_0_5"
    } else if lower.contains("1st inning") || lower.contains("1st_inning") {
        "first_inning"
    } else if lower.contains("2nd inning") || lower.contains("2nd_inning") {
        "second_inning"
    } else if lower.contains("3rd inning") || lower.contains("3rd_inning") {
        "third_inning"
    } else if lower.contains("4th inning") || lower.contains("4th_inning") {
        "fourth_inning"
    } else if lower.contains("5th inning") || lower.contains("5th_inning") {
        "fifth_inning"
    } else if lower.contains("6th inning") || lower.contains("6th_inning") {
        "sixth_inning"
    } else if lower.contains("7th inning") || lower.contains("7th_inning") {
        "seventh_inning"
    } else if lower.contains("8th inning") || lower.contains("8th_inning") {
        "eighth_inning"
    } else if lower.contains("9th inning") || lower.contains("9th_inning") {
        "ninth_inning"
    } else if lower.contains("first 3 innings") || lower.contains("3_innings") {
        "first_3_innings"
    } else if lower.contains("first 5 innings")
        || lower.contains("1st half")
        || lower.contains("first half")
        || lower.contains("1st_half")
    {
        "first_half"
    } else if lower.contains("2nd half") || lower.contains("second half") {
        "second_half"
    } else if lower.contains("first 7 innings") || lower.contains("7_innings") {
        "first_7_innings"
    } else if lower.contains("1st quarter") || lower.contains("1st_quarter") || lower.contains("1q")
    {
        "first_quarter"
    } else if lower.contains("2nd quarter") || lower.contains("2nd_quarter") || lower.contains("2q")
    {
        "second_quarter"
    } else if lower.contains("3rd quarter") || lower.contains("3rd_quarter") || lower.contains("3q")
    {
        "third_quarter"
    } else if lower.contains("4th quarter") || lower.contains("4th_quarter") || lower.contains("4q")
    {
        "fourth_quarter"
    } else if lower.contains("1st period") || lower.contains("1st_period") {
        "first_period"
    } else if lower.contains("2nd period") || lower.contains("2nd_period") {
        "second_period"
    } else if lower.contains("3rd period") || lower.contains("3rd_period") {
        "third_period"
    } else if lower.contains("60 min") || lower.contains("3 way") || lower.contains("3-way") {
        "regulation"
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

fn env_bool(name: &str, default: bool) -> bool {
    match std::env::var(name) {
        Ok(raw) => matches!(
            raw.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        Err(_) => default,
    }
}

fn expanded_market_scan_enabled() -> bool {
    env_bool("EXPANDED_MARKET_SCAN", true)
}

fn expanded_market_max_events() -> usize {
    std::env::var("EXPANDED_MARKET_MAX_EVENTS_PER_SPORT")
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(8)
}

fn pinnacle_period_label(period: u64) -> Option<&'static str> {
    match period {
        0 => Some("full_game"),
        1 => Some("first_half"),
        2 => Some("second_half"),
        3 => Some("first_3_innings"),
        4 => Some("fourth_inning"),
        5 => Some("fifth_inning"),
        6 => Some("regulation"),
        7 => Some("seventh_inning"),
        8 => Some("eighth_inning"),
        9 => Some("ninth_inning"),
        _ => None,
    }
}

#[derive(Clone, Copy)]
enum PinnacleOddsSource {
    Authenticated,
    Guest,
}

impl PinnacleOddsSource {
    fn from_env() -> Self {
        match std::env::var("PIN_ODDS_SOURCE")
            .unwrap_or_else(|_| "auth".to_string())
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "auth" | "authenticated" | "logged-in" | "logged_in" => Self::Authenticated,
            "guest" | "public" => Self::Guest,
            other => {
                tracing::warn!(
                    "Unknown PIN_ODDS_SOURCE='{}'; using authenticated Pinnacle odds",
                    other
                );
                Self::Authenticated
            }
        }
    }

    fn log_label(self) -> &'static str {
        "PIN"
    }

    fn source_name(self) -> &'static str {
        match self {
            Self::Authenticated => "auth",
            Self::Guest => "guest",
        }
    }
}

pub fn pinnacle_odds_source_name() -> &'static str {
    PinnacleOddsSource::from_env().source_name()
}

pub fn pinnacle_auth_odds_base_url() -> String {
    std::env::var("PIN_AUTH_ODDS_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:3404".to_string())
}

pub fn draftkings_proxy_base_url() -> String {
    let port = std::env::var("DK_PROXY_PORT").unwrap_or_else(|_| "3403".to_string());
    format!("http://127.0.0.1:{}", port)
}

fn dedup_markets(markets: Vec<CanonicalMarket>) -> Vec<CanonicalMarket> {
    let mut seen = HashSet::new();
    let mut out = Vec::with_capacity(markets.len());
    for market in markets {
        let key = format!(
            "{}|{}|{}|{}|{}|{}|{}|{}|{}",
            market.venue,
            market.venue_market_id,
            market.selection,
            market.line.map(|v| format!("{:.1}", v)).unwrap_or_default(),
            market.period,
            market.market_type,
            market.surface_key.clone().unwrap_or_default(),
            market.player_name.clone().unwrap_or_default(),
            market.prop_type.clone().unwrap_or_default(),
        );
        if seen.insert(key) {
            out.push(market);
        }
    }
    out
}

fn over_under_selection(label: &str) -> Option<&'static str> {
    let lower = label.trim().to_ascii_lowercase();
    if lower == "o" || lower == "over" || lower.starts_with("over ") || lower.contains(" over") {
        Some("over")
    } else if lower == "u"
        || lower == "under"
        || lower.starts_with("under ")
        || lower.contains(" under")
    {
        Some("under")
    } else {
        None
    }
}

fn extract_first_number(raw: &str) -> Option<f64> {
    let mut buf = String::new();
    let mut started = false;
    for c in raw.chars() {
        if c.is_ascii_digit() || c == '.' || (!started && c == '-') {
            started = true;
            buf.push(c);
        } else if started {
            break;
        }
    }
    if started {
        buf.parse::<f64>().ok()
    } else {
        None
    }
}

fn prop_type_from_text(raw: &str) -> Option<&'static str> {
    let lower = raw.to_ascii_lowercase();
    if lower.contains("points")
        && (lower.contains("rebounds") || lower.contains("reb"))
        && (lower.contains("assists") || lower.contains("ast"))
    {
        Some("pra")
    } else if lower.contains("points") && (lower.contains("rebounds") || lower.contains("reb")) {
        Some("pr")
    } else if lower.contains("points") && (lower.contains("assists") || lower.contains("ast")) {
        Some("pa")
    } else if (lower.contains("rebounds") || lower.contains("reb"))
        && (lower.contains("assists") || lower.contains("ast"))
    {
        Some("ra")
    } else if lower.contains("made threes")
        || lower.contains("3 point")
        || lower.contains("3-point")
        || lower.contains("three")
        || lower.contains("threes")
    {
        Some("threes")
    } else if lower.contains("points") || lower.contains("pts") {
        Some("points")
    } else if lower.contains("rebound") || lower.contains("reb") {
        Some("rebounds")
    } else if lower.contains("assist") || lower.contains("ast") {
        Some("assists")
    } else if lower.contains("block") {
        Some("blocks")
    } else if lower.contains("steal") {
        Some("steals")
    } else if lower.contains("save") {
        Some("saves")
    } else if lower.contains("shot") && lower.contains("goal") {
        Some("sog")
    } else if lower.contains("goal") {
        Some("goals")
    } else if lower.contains("strikeout") {
        Some("strikeouts")
    } else if lower.contains("pitching outs") || lower.contains("outs recorded") {
        Some("pitching_outs")
    } else if lower.contains("hits allowed") {
        Some("hits_allowed")
    } else if lower.contains("earned runs") {
        Some("earned_runs")
    } else if lower.contains("total bases") {
        Some("total_bases")
    } else if lower.contains("home runs") {
        Some("home_runs")
    } else if lower.contains("runs") {
        Some("runs")
    } else {
        None
    }
}

fn player_surface_key(prop_type: &str, player_name: &str, period: &str) -> String {
    format!("player:{}:{}:{}", prop_type, player_name, period)
}

fn team_total_surface_key(team: &str) -> String {
    format!("team_total:{}", team)
}

pub async fn fetch_all(client: &Client) -> Vec<CanonicalMarket> {
    let sports = config::sports();
    let mut handles: Vec<tokio::task::JoinHandle<Vec<CanonicalMarket>>> = Vec::new();
    let pin_source = PinnacleOddsSource::from_env();

    for sport in &sports {
        // DraftKings — spawn concurrent
        if let Some(dk_id) = sport.dk_id {
            let c = client.clone();
            let name = sport.name;
            handles.push(tokio::spawn(async move {
                match fetch_draftkings(&c, name, dk_id).await {
                    Ok(markets) => {
                        tracing::info!("[DK]  {:<10} → {} markets", name, markets.len());
                        markets
                    }
                    Err(e) => {
                        tracing::warn!("[DK]  {:<10} fetch failed: {}", name, e);
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
                        tracing::info!("[FD]  {:<10} → {} markets", name, markets.len());
                        markets
                    }
                    Err(e) => {
                        tracing::warn!("[FD]  {:<10} fetch failed: {}", name, e);
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
                        tracing::info!("[FD]  {:<10} → {} markets", name, markets.len());
                        markets
                    }
                    Err(e) => {
                        tracing::warn!("[FD]  {:<10} fetch failed: {}", name, e);
                        Vec::new()
                    }
                }
            }));
        }

        // Pinnacle — spawn concurrent
        for &league_id in &sport.pin_league_ids {
            let c = client.clone();
            let name = sport.name;
            let source = pin_source;
            handles.push(tokio::spawn(async move {
                match fetch_pinnacle(&c, name, league_id, source).await {
                    Ok(markets) => {
                        tracing::info!(
                            "[{}] {:<10} → {} markets",
                            source.log_label(),
                            name,
                            markets.len()
                        );
                        markets
                    }
                    Err(e) => {
                        tracing::warn!("[{}] {:<10} fetch failed: {}", source.log_label(), name, e);
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
    dedup_markets(all)
}

// ── DraftKings ──────────────────────────────────────────────────────────────

async fn fetch_draftkings(
    client: &Client,
    sport: &str,
    group_id: u64,
) -> Result<Vec<CanonicalMarket>, String> {
    // Use local Node.js proxy (dk-proxy.js on port 3403) to work around
    // reqwest TLS incompatibility with DK's CDN on Windows.
    let proxy_url = format!("{}/dk/{}", draftkings_proxy_base_url(), group_id);

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
        config::DK_HOSTNAME,
        config::DK_SITE_CODE,
        group_id
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
        .header(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
        )
        .header("Accept", "application/json")
        .timeout(odds_fetch_timeout())
        .send()
        .await
        .map_err(|e| format!("request failed: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("status {}", resp.status()));
    }

    let json: Value = resp
        .json()
        .await
        .map_err(|e| format!("json parse: {}", e))?;
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
    let mut events_map: std::collections::HashMap<String, &Value> =
        std::collections::HashMap::new();
    for ev in events {
        if let Some(id) = ev.get("id") {
            events_map.insert(val_str(id), ev);
        }
    }

    // Build market lookup by eventId, filter for main markets we care about
    let mut markets_by_event: std::collections::HashMap<String, Vec<&Value>> =
        std::collections::HashMap::new();
    for mkt in dk_markets {
        // The DK league feed only ever contains main markets (main=true: moneyline/run line/total).
        // Alt ladders and player props live on the per-EVENT feed, not here — see the per-event
        // fetcher for those. Keep this loop main-only.
        let is_main = mkt.get("main").and_then(|v| v.as_bool()).unwrap_or(false);
        if !is_main {
            continue;
        }
        let name = mkt.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let mkt_type = resolve_dk_market_type(name, is_main);
        if mkt_type.is_empty() {
            continue;
        }
        if let Some(eid) = mkt.get("eventId") {
            markets_by_event.entry(val_str(eid)).or_default().push(mkt);
        }
    }

    // Build selection lookup by marketId
    let mut sels_by_market: std::collections::HashMap<String, Vec<&Value>> =
        std::collections::HashMap::new();
    for sel in selections {
        if let Some(mid) = sel.get("marketId") {
            sels_by_market.entry(val_str(mid)).or_default().push(sel);
        }
    }

    let mut event_contexts = Vec::new();

    for (eid_str, event) in &events_map {
        let event_name = event.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let start_time = event
            .get("startEventDate")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let is_live = event.get("status").and_then(|v| v.as_str()) == Some("STARTED");

        let participants = event.get("participants").and_then(|v| v.as_array());
        let (dk_away, dk_home) = if let Some(parts) = participants {
            let away = parts
                .iter()
                .find(|p| p.get("venueRole").and_then(|v| v.as_str()) == Some("Away"))
                .or_else(|| parts.first());
            let home = parts
                .iter()
                .find(|p| p.get("venueRole").and_then(|v| v.as_str()) == Some("Home"))
                .or_else(|| parts.get(1));
            let away_name = away
                .and_then(|p| p.get("name").and_then(|v| v.as_str()))
                .unwrap_or("");
            let home_name = home
                .and_then(|p| p.get("name").and_then(|v| v.as_str()))
                .unwrap_or("");
            (away_name.to_string(), home_name.to_string())
        } else {
            parse_dk_event_teams(event_name)
        };

        let dk_event_id = team_names::canonical_event_id(sport, &dk_away, &dk_home, start_time);
        event_contexts.push(DkEventContext {
            event_id: eid_str.clone(),
            canonical_event_id: dk_event_id.clone(),
            start_time: start_time.to_string(),
            is_live,
        });

        for mkt in markets_by_event.get(eid_str).unwrap_or(&Vec::new()) {
            let mkt_name = mkt.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let mkt_is_main = mkt.get("main").and_then(|v| v.as_bool()).unwrap_or(false);
            let market_type = resolve_dk_market_type(mkt_name, mkt_is_main);
            let mkt_id = mkt.get("id").map(|v| val_str(v)).unwrap_or_default();

            for sel in sels_by_market.get(&mkt_id).unwrap_or(&Vec::new()) {
                let label = sel.get("label").and_then(|v| v.as_str()).unwrap_or("");
                let sel_id = sel.get("id").and_then(|v| v.as_str()).unwrap_or(""); // e.g., "0ML84048103_3"

                let dec = sel
                    .get("displayOdds")
                    .and_then(|d| d.get("decimal"))
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse::<f64>().ok())
                    .or_else(|| sel.get("trueOdds").and_then(|v| v.as_f64()));
                let american = sel
                    .get("displayOdds")
                    .and_then(|d| d.get("american"))
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.replace('−', "-").replace('+', "").parse::<f64>().ok());

                let decimal = match dec {
                    Some(d) if d > 1.0 && d < 100.0 => d,
                    _ => continue,
                };

                let line = sel.get("points").and_then(|v| v.as_f64());
                let sel_normalized = team_names::normalize_selection(&market_type, label, sport);
                let surface_key = if market_type == "team_total" {
                    dk_selection_team_name(sel)
                        .or_else(|| mkt_name.split(':').next().map(|s| s.trim().to_string()))
                        .map(|team| {
                            team_total_surface_key(&team_names::normalize_team(&team, sport))
                        })
                } else {
                    None
                };

                out.push(CanonicalMarket {
                    event_id: dk_event_id.clone(),
                    sport: sport.to_string(),
                    league: sport.to_string(),
                    start_time_utc: start_time.to_string(),
                    market_type: market_type.clone(),
                    surface_key,
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

    if expanded_market_scan_enabled() {
        out.extend(fetch_dk_event_subcategories(client, sport, &event_contexts, ts).await);
    }

    Ok(out)
}

#[derive(Clone)]
struct DkEventContext {
    event_id: String,
    canonical_event_id: String,
    start_time: String,
    is_live: bool,
}

const DK_MLB_EVENT_SUBCATEGORIES: &[u64] = &[6686, 11024, 16209, 17319, 17323];
const DK_NBA_NHL_EVENT_SUBCATEGORIES: &[u64] = &[
    4741, 4999, 6353, 10285, 11001, 11567, 12040, 12057, 13031, 13748, 13750, 14495, 16213, 16215,
    16257, 16387, 16388, 16389, 16477, 16479, 16544, 16545, 16546, 16548, 16550, 18002, 18005,
    18010, 18750, 18757, 18758, 18777,
];

fn dk_event_subcategory_ids(sport: &str) -> &'static [u64] {
    match sport {
        "MLB" => DK_MLB_EVENT_SUBCATEGORIES,
        "NBA" | "NHL" => DK_NBA_NHL_EVENT_SUBCATEGORIES,
        _ => &[],
    }
}

async fn fetch_dk_event_subcategories(
    client: &Client,
    sport: &str,
    events: &[DkEventContext],
    ts: u64,
) -> Vec<CanonicalMarket> {
    let subcategories = dk_event_subcategory_ids(sport);
    if subcategories.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    for event in events.iter().take(expanded_market_max_events()) {
        for subcategory_id in subcategories {
            let proxy_url = format!(
                "{}/dk-event/{}/{}",
                draftkings_proxy_base_url(),
                event.event_id,
                subcategory_id
            );
            match fetch_dk_event_subcategory_inner(client, &proxy_url, sport, event, ts).await {
                Ok(markets) => {
                    out.extend(markets);
                    continue;
                }
                Err(e) => tracing::debug!(
                    "[DK] event subcategory proxy failed event={} subcat={} err={}",
                    event.event_id,
                    subcategory_id,
                    e
                ),
            }

            let query = format!(
                "%24filter%3DeventId%20eq%20%27{}%27%20AND%20clientMetadata%2FsubCategoryId%20eq%20%27{}%27%20AND%20tags%2Fall%28t%3A%20t%20ne%20%27SportcastBetBuilder%27%29",
                event.event_id, subcategory_id
            );
            let direct_url = format!(
                "https://{}/sites/{}/api/sportscontent/controldata/event/eventSubcategory/v1/markets?isBatchable=false&templateVars={}%2C{}&marketsQuery={}&entity=markets",
                config::DK_HOSTNAME,
                config::DK_SITE_CODE,
                event.event_id,
                subcategory_id,
                query
            );
            match fetch_dk_event_subcategory_inner(client, &direct_url, sport, event, ts).await {
                Ok(markets) => out.extend(markets),
                Err(e) => tracing::debug!(
                    "[DK] event subcategory direct failed event={} subcat={} err={}",
                    event.event_id,
                    subcategory_id,
                    e
                ),
            }
        }
    }

    out
}

async fn fetch_dk_event_subcategory_inner(
    client: &Client,
    url: &str,
    sport: &str,
    event: &DkEventContext,
    ts: u64,
) -> Result<Vec<CanonicalMarket>, String> {
    let resp = client
        .get(url)
        .header(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
        )
        .header("Accept", "application/json")
        .timeout(odds_fetch_timeout())
        .send()
        .await
        .map_err(|e| format!("request failed: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("status {}", resp.status()));
    }

    let json: Value = resp
        .json()
        .await
        .map_err(|e| format!("json parse: {}", e))?;
    Ok(parse_dk_event_subcategory_response(&json, sport, event, ts))
}

fn parse_dk_event_subcategory_response(
    json: &Value,
    sport: &str,
    event: &DkEventContext,
    ts: u64,
) -> Vec<CanonicalMarket> {
    let mut out = Vec::new();
    let markets = match json.get("markets").and_then(|v| v.as_array()) {
        Some(markets) => markets,
        None => return out,
    };
    let selections = match json.get("selections").and_then(|v| v.as_array()) {
        Some(selections) => selections,
        None => return out,
    };

    let mut selections_by_market: HashMap<String, Vec<&Value>> = HashMap::new();
    for selection in selections {
        if let Some(market_id) = selection.get("marketId").and_then(value_to_string) {
            selections_by_market
                .entry(market_id)
                .or_default()
                .push(selection);
        }
    }

    for market in markets {
        let market_id = match market.get("id").and_then(value_to_string) {
            Some(id) => id,
            None => continue,
        };
        let market_name = market.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let market_type_name = market
            .get("marketType")
            .and_then(|v| v.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or(market_name);
        let combined_label = format!("{} {}", market_name, market_type_name);
        let period = detect_period(&combined_label);
        let base_market_type = resolve_dk_market_type(&combined_label, false);
        let selections_for_market = selections_by_market.get(&market_id);
        let selections_for_market = match selections_for_market {
            Some(selections) => selections,
            None => continue,
        };
        let has_player_participants = selections_for_market
            .iter()
            .any(|selection| dk_selection_player_name(selection).is_some());
        let prop_type = if has_player_participants {
            prop_type_from_text(&combined_label)
        } else {
            None
        };

        for selection in selections_for_market {
            let selection_label = selection
                .get("label")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let selection_id = selection.get("id").and_then(|v| v.as_str()).unwrap_or("");
            if selection_id.is_empty() {
                continue;
            }

            let decimal = match dk_selection_decimal(selection) {
                Some(decimal) if decimal > 1.0 && decimal < 100.0 => decimal,
                _ => continue,
            };
            let american = dk_selection_american(selection);
            let points = selection
                .get("points")
                .and_then(|v| v.as_f64())
                .or_else(|| extract_first_number(selection_label));

            if let Some(prop_type) = prop_type {
                let selection_side = match over_under_selection(selection_label) {
                    Some(side) => side,
                    None => continue,
                };
                let line = match points {
                    Some(line) => line,
                    None => continue,
                };
                let player_raw = match dk_selection_player_name(selection) {
                    Some(name) => name,
                    None => continue,
                };
                let player_name = team_names::normalize_player(&player_raw);
                if player_name.is_empty() {
                    continue;
                }
                out.push(CanonicalMarket {
                    event_id: event.canonical_event_id.clone(),
                    sport: sport.to_string(),
                    league: sport.to_string(),
                    start_time_utc: event.start_time.clone(),
                    market_type: "player_prop".to_string(),
                    surface_key: Some(player_surface_key(prop_type, &player_name, period)),
                    selection: selection_side.to_string(),
                    line: Some(line),
                    period: period.to_string(),
                    is_live: event.is_live,
                    player_name: Some(player_name),
                    prop_type: Some(prop_type.to_string()),
                    decimal_odds: decimal,
                    american_odds: american,
                    venue: "draftkings".to_string(),
                    venue_market_id: selection_id.to_string(),
                    venue_event_id: format!("dk:{}", event.event_id),
                    received_at_ms: ts,
                });
                continue;
            }

            if base_market_type.is_empty() {
                continue;
            }

            let mut surface_key = None;
            let normalized_selection = if base_market_type == "total" {
                match over_under_selection(selection_label) {
                    Some(side) => side.to_string(),
                    None => continue,
                }
            } else if base_market_type == "team_total" {
                let side = match over_under_selection(selection_label) {
                    Some(side) => side.to_string(),
                    None => continue,
                };
                if let Some(team_name) = dk_selection_team_name(selection)
                    .or_else(|| market_name.split(':').next().map(|s| s.trim().to_string()))
                {
                    let team = team_names::normalize_team(&team_name, sport);
                    surface_key = Some(team_total_surface_key(&team));
                }
                side
            } else {
                team_names::normalize_selection(&base_market_type, selection_label, sport)
            };

            if (base_market_type == "spread"
                || base_market_type == "total"
                || base_market_type == "team_total")
                && points.is_none()
            {
                continue;
            }

            out.push(CanonicalMarket {
                event_id: event.canonical_event_id.clone(),
                sport: sport.to_string(),
                league: sport.to_string(),
                start_time_utc: event.start_time.clone(),
                market_type: base_market_type.clone(),
                surface_key,
                selection: normalized_selection,
                line: points,
                period: period.to_string(),
                is_live: event.is_live,
                player_name: None,
                prop_type: None,
                decimal_odds: decimal,
                american_odds: american,
                venue: "draftkings".to_string(),
                venue_market_id: selection_id.to_string(),
                venue_event_id: format!("dk:{}", event.event_id),
                received_at_ms: ts,
            });
        }
    }

    out
}

fn dk_selection_decimal(selection: &Value) -> Option<f64> {
    selection
        .get("displayOdds")
        .and_then(|d| d.get("decimal"))
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<f64>().ok())
        .or_else(|| selection.get("trueOdds").and_then(|v| v.as_f64()))
}

fn dk_selection_american(selection: &Value) -> Option<f64> {
    selection
        .get("displayOdds")
        .and_then(|d| d.get("american"))
        .and_then(|v| v.as_str())
        .and_then(parse_american_string)
}

fn parse_american_string(raw: &str) -> Option<f64> {
    raw.replace('\u{2212}', "-")
        .replace('+', "")
        .parse::<f64>()
        .ok()
}

fn dk_selection_player_name(selection: &Value) -> Option<String> {
    selection
        .get("participants")
        .and_then(|v| v.as_array())
        .and_then(|participants| {
            participants
                .iter()
                .find(|p| p.get("type").and_then(|v| v.as_str()) == Some("Player"))
                .or_else(|| participants.first())
        })
        .and_then(|p| p.get("name").and_then(|v| v.as_str()))
        .map(str::to_string)
}

fn dk_selection_team_name(selection: &Value) -> Option<String> {
    selection
        .get("participants")
        .and_then(|v| v.as_array())
        .and_then(|participants| {
            participants
                .iter()
                .find(|p| p.get("type").and_then(|v| v.as_str()) == Some("Team"))
                .or_else(|| participants.first())
        })
        .and_then(|p| p.get("name").and_then(|v| v.as_str()))
        .map(str::to_string)
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
    let start_time = event
        .get("startDate")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let is_live = event
        .get("eventStatus")
        .and_then(|v| v.get("state"))
        .and_then(|v| v.as_str())
        .map(|s| s == "STARTED")
        .unwrap_or(false);

    let label = offer.get("label").and_then(|v| v.as_str()).unwrap_or("");
    let market_type = resolve_dk_market_type(label, true); // offer API markets are main
    if market_type.is_empty() {
        return;
    }

    // Parse home/away from event name (DK format: "Away at/vs Home" or "Away @ Home")
    let (dk_away, dk_home) = parse_dk_event_teams(event_name);
    let dk_event_id = team_names::canonical_event_id(sport, &dk_away, &dk_home, start_time);
    let offer_id = offer
        .get("providerOfferId")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if let Some(outcomes) = offer.get("outcomes").and_then(|v| v.as_array()) {
        for outcome in outcomes {
            let selection = outcome.get("label").and_then(|v| v.as_str()).unwrap_or("");
            let odds_decimal = outcome
                .get("oddsDecimal")
                .and_then(|v| v.as_f64())
                .or_else(|| outcome.get("odds").and_then(|v| v.as_f64()));
            let odds_american = outcome
                .get("oddsAmerican")
                .and_then(|v| v.as_str())
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

            let line = outcome
                .get("line")
                .and_then(|v| v.as_f64())
                .or_else(|| outcome.get("points").and_then(|v| v.as_f64()));

            let sel_normalized = team_names::normalize_selection(&market_type, selection, sport);
            let surface_key = if market_type == "team_total" {
                label
                    .split(':')
                    .next()
                    .map(|team| team_total_surface_key(&team_names::normalize_team(team, sport)))
            } else {
                None
            };

            out.push(CanonicalMarket {
                event_id: dk_event_id.clone(),
                sport: sport.to_string(),
                league: sport.to_string(),
                start_time_utc: start_time.to_string(),
                market_type: market_type.clone(),
                surface_key,
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

fn resolve_dk_market_type(label: &str, _is_main: bool) -> String {
    let lower = label.to_lowercase();
    if lower.contains("moneyline") || lower == "money line" {
        "moneyline".to_string()
    } else if lower.contains("spread")
        || lower.contains("point spread")
        || lower.contains("run line")
        || lower.contains("puck line")
    {
        // Main and alternate spreads (incl. MLB run line / NHL puck line) collapse to one type so a
        // common line that is "main" on one book and "alt" on another still shares an identity
        // bucket and is matched purely by line.
        "spread".to_string()
    } else if lower.contains("total") && lower.contains("team") {
        "team_total".to_string()
    } else if lower.contains("total")
        || lower.contains("over/under")
        || lower.contains("o/u")
        || (lower.contains("runs") && lower.contains("inning"))
        || (lower.contains("goals") && lower.contains("period"))
    {
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
            .header(
                "User-Agent",
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
            )
            .header("Accept", "application/json")
            .timeout(odds_fetch_timeout())
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
                let mut markets = parse_fd_response(&json, sport)?;
                if expanded_market_scan_enabled() {
                    let event_ids = fd_event_ids(&json);
                    markets
                        .extend(fetch_fanduel_event_pages(client, sport, state, &event_ids).await);
                }
                return Ok(dedup_markets(markets));
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
            .header(
                "User-Agent",
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
            )
            .header("Accept", "application/json")
            .timeout(odds_fetch_timeout())
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
                let mut markets = parse_fd_response(&json, sport)?;
                if expanded_market_scan_enabled() {
                    let event_ids = fd_event_ids(&json);
                    markets
                        .extend(fetch_fanduel_event_pages(client, sport, state, &event_ids).await);
                }
                return Ok(dedup_markets(markets));
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

fn fd_event_ids(json: &Value) -> Vec<String> {
    json.get("attachments")
        .and_then(|attachments| attachments.get("events"))
        .and_then(|events| events.as_object())
        .map(|events| {
            events
                .keys()
                .take(expanded_market_max_events())
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

async fn fetch_fanduel_event_pages(
    client: &Client,
    sport: &str,
    state: &str,
    event_ids: &[String],
) -> Vec<CanonicalMarket> {
    let mut out = Vec::new();
    for event_id in event_ids.iter().take(expanded_market_max_events()) {
        let url = format!(
            "https://sbapi.{}.sportsbook.fanduel.ca/api/event-page?_ak={}&eventId={}&useCombinedTouchdownsVirtualMarket=true&useQuickBets=true&useQuickBetsMLB=true",
            state,
            config::FD_API_KEY,
            event_id
        );
        let resp = match client
            .get(&url)
            .header(
                "User-Agent",
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
            )
            .header("Accept", "application/json")
            .timeout(odds_fetch_timeout())
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => resp,
            Ok(resp) => {
                tracing::debug!(
                    "[FD] event-page {} event={} status={}",
                    sport,
                    event_id,
                    resp.status()
                );
                continue;
            }
            Err(e) => {
                tracing::debug!("[FD] event-page {} event={} err={}", sport, event_id, e);
                continue;
            }
        };

        match resp.json::<Value>().await {
            Ok(json) if json.get("attachments").is_some() => {
                match parse_fd_response(&json, sport) {
                    Ok(markets) => out.extend(markets),
                    Err(e) => tracing::debug!(
                        "[FD] event-page parse {} event={} err={}",
                        sport,
                        event_id,
                        e
                    ),
                }
            }
            Ok(_) => {}
            Err(e) => tracing::debug!(
                "[FD] event-page json {} event={} err={}",
                sport,
                event_id,
                e
            ),
        }
    }
    out
}

fn parse_fd_response(json: &Value, sport: &str) -> Result<Vec<CanonicalMarket>, String> {
    let ts = now_ms();
    let mut markets = Vec::new();

    let attachments = match json.get("attachments") {
        Some(a) => a,
        None => return Ok(markets),
    };

    // Build event lookup
    let mut events_map: std::collections::HashMap<String, &Value> =
        std::collections::HashMap::new();
    if let Some(events) = attachments.get("events").and_then(|v| v.as_object()) {
        for (k, v) in events {
            events_map.insert(k.clone(), v);
        }
    }
    let competition_names = fd_competition_names(attachments);

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
            let resolved_sport =
                match resolve_fd_event_sport(sport, event.copied(), &competition_names) {
                    Some(s) => s,
                    None => continue,
                };

            let market_name = market
                .get("marketName")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let market_type_raw = market
                .get("marketType")
                .and_then(|v| v.as_str())
                .unwrap_or(market_name);
            let market_type = resolve_fd_market_type(market_type_raw, market_name);
            if market_type.is_empty() {
                continue;
            }
            let fd_prop_type = if market_type == "player_prop" {
                prop_type_from_text(&format!("{} {}", market_name, market_type_raw))
            } else {
                None
            };

            // Parse FD event name: "Team A @ Team B" or "Team A v Team B"
            let (fd_away, fd_home) = parse_fd_event_teams(event_name);
            // Skip futures/props/awards that don't parse into two teams
            if fd_home == "unknown" || fd_away == "unknown" {
                continue;
            }
            let fd_event_id =
                team_names::canonical_event_id(&resolved_sport, &fd_away, &fd_home, start_time);

            if let Some(runners) = market.get("runners").and_then(|v| v.as_array()) {
                for runner in runners {
                    let runner_name = runner
                        .get("runnerName")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
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

                    let period = detect_period(&format!("{} {}", market_name, market_type_raw));
                    let mut surface_key = None;
                    let mut player_name = None;
                    let mut prop_type = None;
                    let (sel_normalized, line) = if market_type == "player_prop" {
                        let selection = match over_under_selection(runner_name) {
                            Some(selection) => selection.to_string(),
                            None => continue,
                        };
                        let prop = match fd_prop_type {
                            Some(prop) => prop,
                            None => continue,
                        };
                        let line = match fd_runner_line(
                            handicap,
                            market_name,
                            market_type_raw,
                            runner_name,
                        ) {
                            Some(line) => line,
                            None => continue,
                        };
                        let player_raw = match fd_player_name(market_name, runner_name) {
                            Some(player) => player,
                            None => continue,
                        };
                        let normalized_player = team_names::normalize_player(&player_raw);
                        if normalized_player.is_empty() {
                            continue;
                        }
                        surface_key = Some(player_surface_key(prop, &normalized_player, period));
                        player_name = Some(normalized_player);
                        prop_type = Some(prop.to_string());
                        (selection, Some(line))
                    } else if market_type == "total" || market_type == "team_total" {
                        let selection = match over_under_selection(runner_name) {
                            Some(selection) => selection.to_string(),
                            None => continue,
                        };
                        let line = match fd_runner_line(
                            handicap,
                            market_name,
                            market_type_raw,
                            runner_name,
                        ) {
                            Some(line) => line,
                            None => continue,
                        };
                        if market_type == "team_total" {
                            if let Some(team) = fd_team_total_team(market_name, runner_name) {
                                let normalized_team =
                                    team_names::normalize_team(&team, &resolved_sport);
                                surface_key = Some(team_total_surface_key(&normalized_team));
                            }
                        }
                        (selection, Some(line))
                    } else {
                        (
                            team_names::normalize_selection(
                                &market_type,
                                runner_name,
                                &resolved_sport,
                            ),
                            handicap,
                        )
                    };

                    // Build adapter-ready selection ID: "marketId:selectionId"
                    let market_id_str = market
                        .get("marketId")
                        .and_then(|v| {
                            if v.is_string() {
                                v.as_str().map(|s| s.to_string())
                            } else {
                                v.as_u64().map(|n| n.to_string())
                            }
                        })
                        .unwrap_or_default();
                    let selection_id_str = runner
                        .get("selectionId")
                        .and_then(|v| {
                            if v.is_string() {
                                v.as_str().map(|s| s.to_string())
                            } else {
                                v.as_u64().map(|n| n.to_string())
                            }
                        })
                        .unwrap_or_default();
                    let adapter_sel_id = format!("{}:{}", market_id_str, selection_id_str);

                    out_push_fd(
                        &mut markets,
                        &fd_event_id,
                        &resolved_sport,
                        &event_id_str,
                        start_time,
                        &market_type,
                        surface_key,
                        &sel_normalized,
                        line,
                        decimal,
                        american,
                        is_live,
                        &adapter_sel_id,
                        player_name,
                        prop_type,
                        ts,
                        period,
                    );
                }
            }
        }
    }

    Ok(markets)
}

fn fd_competition_names(attachments: &Value) -> HashMap<String, String> {
    let mut names = HashMap::new();
    if let Some(competitions) = attachments.get("competitions").and_then(|v| v.as_object()) {
        for (id, competition) in competitions {
            if let Some(name) = competition.get("name").and_then(|v| v.as_str()) {
                names.insert(id.clone(), name.to_string());
                if let Some(comp_id) = competition.get("competitionId") {
                    if let Some(comp_id) = value_to_string(comp_id) {
                        names.insert(comp_id, name.to_string());
                    }
                }
            }
        }
    }
    names
}

fn resolve_fd_event_sport(
    configured_sport: &str,
    event: Option<&Value>,
    competition_names: &HashMap<String, String>,
) -> Option<String> {
    if configured_sport != "SOCCER" {
        return Some(configured_sport.to_string());
    }

    let event = event?;
    let competition_name = event
        .get("competitionId")
        .and_then(value_to_string)
        .and_then(|id| competition_names.get(&id).cloned())
        .or_else(|| {
            event
                .get("competitionName")
                .and_then(|v| v.as_str())
                .map(str::to_string)
        })?;

    map_fd_soccer_competition(&competition_name).map(str::to_string)
}

fn value_to_string(value: &Value) -> Option<String> {
    if let Some(s) = value.as_str() {
        Some(s.to_string())
    } else if let Some(n) = value.as_u64() {
        Some(n.to_string())
    } else if let Some(n) = value.as_i64() {
        Some(n.to_string())
    } else {
        None
    }
}

fn map_fd_soccer_competition(name: &str) -> Option<&'static str> {
    let normalized = name.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "english premier league" | "england premier league" | "premier league" => Some("EPL"),
        "spanish la liga" | "spain la liga" | "la liga" => Some("LA_LIGA"),
        "italian serie a" | "serie a" => Some("SERIE_A"),
        "german bundesliga" | "bundesliga" => Some("BUNDESLIGA"),
        "french ligue 1" | "ligue 1" => Some("LIGUE_1"),
        "uefa champions league" | "champions league" => Some("UCL"),
        "us mls" | "usa mls" | "major league soccer" | "mls" => Some("MLS"),
        _ => None,
    }
}

fn out_push_fd(
    out: &mut Vec<CanonicalMarket>,
    event_id: &str,
    sport: &str,
    fd_event_id: &str,
    start_time: &str,
    market_type: &str,
    surface_key: Option<String>,
    selection: &str,
    line: Option<f64>,
    decimal: f64,
    american: Option<f64>,
    is_live: bool,
    adapter_sel_id: &str,
    player_name: Option<String>,
    prop_type: Option<String>,
    ts: u64,
    period: &str,
) {
    out.push(CanonicalMarket {
        event_id: event_id.to_string(),
        sport: sport.to_string(),
        league: sport.to_string(),
        start_time_utc: start_time.to_string(),
        market_type: market_type.to_string(),
        surface_key,
        selection: selection.to_string(),
        line,
        period: period.to_string(),
        is_live,
        player_name,
        prop_type,
        decimal_odds: decimal,
        american_odds: american,
        venue: "fanduel".to_string(),
        venue_market_id: adapter_sel_id.to_string(),
        venue_event_id: format!("fd:{}", fd_event_id),
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

fn fd_runner_line(
    handicap: Option<f64>,
    market_name: &str,
    market_type: &str,
    runner_name: &str,
) -> Option<f64> {
    handicap
        .filter(|value| *value != 0.0)
        .or_else(|| {
            let lower_runner = runner_name.to_ascii_lowercase();
            if lower_runner.contains(" over ") || lower_runner.contains(" under ") {
                lower_runner
                    .split(" over ")
                    .nth(1)
                    .or_else(|| lower_runner.split(" under ").nth(1))
                    .and_then(extract_first_number)
            } else {
                None
            }
        })
        .or_else(|| extract_first_number(market_name))
        .or_else(|| extract_first_number(market_type))
}

fn fd_player_name(market_name: &str, runner_name: &str) -> Option<String> {
    if let Some((player, _)) = market_name.split_once(" - ") {
        let player = player.trim();
        if !player.is_empty() {
            return Some(player.to_string());
        }
    }

    let lower = runner_name.to_ascii_lowercase();
    for token in [" over", " under"] {
        if let Some(idx) = lower.find(token) {
            let player = runner_name[..idx].trim();
            if !player.is_empty() {
                return Some(player.to_string());
            }
        }
    }
    None
}

fn fd_team_total_team(market_name: &str, runner_name: &str) -> Option<String> {
    for raw in [runner_name, market_name] {
        let lower = raw.to_ascii_lowercase();
        for token in [" over", " under", " total"] {
            if let Some(idx) = lower.find(token) {
                let team = raw[..idx].trim();
                if !team.is_empty() {
                    return Some(team.to_string());
                }
            }
        }
    }
    None
}

fn fd_unsupported_market(market_type: &str, market_name: &str) -> bool {
    let lower = format!("{} {}", market_type, market_name).to_ascii_lowercase();
    lower.contains("odd/even")
        || lower.contains("bands")
        || lower.contains("double")
        || lower.contains("parlay")
        || lower.contains("tri-bet")
        || lower.contains("race_to")
        || lower.contains("race to")
        || lower.contains("exact")
        || lower.contains("correct_score")
        || lower.contains("winning_margin")
}

fn resolve_fd_market_type(market_type: &str, market_name: &str) -> String {
    let lower_type = market_type.to_lowercase();
    let lower_name = market_name.to_lowercase();
    if fd_unsupported_market(market_type, market_name) {
        String::new()
    } else if lower_type.contains("player_")
        || lower_type.contains("pitcher_")
        || (lower_name.contains(" - ") && prop_type_from_text(&lower_name).is_some())
    {
        "player_prop".to_string()
    } else if lower_type.contains("moneyline")
        || lower_name.contains("moneyline")
        || lower_type.contains("match_betting")
        || lower_type.contains("match betting")
    {
        "moneyline".to_string()
    } else if lower_type.contains("spread")
        || lower_name.contains("spread")
        || lower_type.contains("handicap")
        || lower_type.contains("run_line")
        || lower_name.contains("run line")
        || lower_type.contains("puck_line")
        || lower_name.contains("puck line")
    {
        "spread".to_string()
    } else if lower_type.contains("team_total")
        || lower_name.contains("team total")
        || lower_type.contains("home_total_runs")
        || lower_type.contains("away_total_runs")
        || lower_name.ends_with("total runs")
    {
        "team_total".to_string()
    } else if lower_type.contains("total")
        || lower_name.contains("total")
        || lower_name.contains("over/under")
        || lower_type.contains("over/under")
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
    source: PinnacleOddsSource,
) -> Result<Vec<CanonicalMarket>, String> {
    match source {
        PinnacleOddsSource::Authenticated => {
            fetch_pinnacle_authenticated(client, sport, league_id).await
        }
        PinnacleOddsSource::Guest => fetch_pinnacle_guest(client, sport, league_id).await,
    }
}

async fn fetch_pinnacle_authenticated(
    client: &Client,
    sport: &str,
    league_id: u64,
) -> Result<Vec<CanonicalMarket>, String> {
    let base = pinnacle_auth_odds_base_url();
    let url = format!(
        "{}/pinnacle/snapshot/{}",
        base.trim_end_matches('/'),
        league_id
    );

    let resp = client
        .get(&url)
        .header("Accept", "application/json")
        .timeout(odds_fetch_timeout())
        .send()
        .await
        .map_err(|e| format!("authenticated snapshot request failed: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        let preview: String = body.chars().take(300).collect();
        return Err(format!(
            "authenticated snapshot status {}: {}",
            status, preview
        ));
    }

    let json: Value = resp
        .json()
        .await
        .map_err(|e| format!("authenticated snapshot json parse: {}", e))?;
    let matchups_json = json
        .get("matchups")
        .ok_or_else(|| format!("authenticated snapshot {} missing matchups", sport))?;
    let markets_json = json
        .get("markets")
        .ok_or_else(|| format!("authenticated snapshot {} missing markets", sport))?;

    let ts = now_ms();
    let mut out = parse_pinnacle_markets(sport, matchups_json, markets_json, ts)?;
    if expanded_market_scan_enabled() {
        if let (Some(related_matchups), Some(related_markets)) =
            (json.get("related_matchups"), json.get("related_markets"))
        {
            out.extend(parse_pinnacle_related_markets(
                sport,
                matchups_json,
                related_matchups,
                related_markets,
                ts,
            )?);
        }
    }
    Ok(dedup_markets(out))
}

async fn fetch_pinnacle_guest(
    client: &Client,
    sport: &str,
    league_id: u64,
) -> Result<Vec<CanonicalMarket>, String> {
    // Step 1: Fetch matchups (events)
    let matchups_url = format!(
        "https://{}/0.1/leagues/{}/matchups",
        config::PIN_DOMAIN,
        league_id
    );
    let matchups_resp = client
        .get(&matchups_url)
        .header(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
        )
        .timeout(odds_fetch_timeout())
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !matchups_resp.status().is_success() {
        return Err(format!(
            "PIN matchups {} status: {}",
            sport,
            matchups_resp.status()
        ));
    }

    let matchups_json: Value = matchups_resp.json().await.map_err(|e| e.to_string())?;

    // Step 2: Fetch markets (odds)
    let markets_url = format!(
        "https://{}/0.1/leagues/{}/markets/straight",
        config::PIN_DOMAIN,
        league_id
    );
    let markets_resp = client
        .get(&markets_url)
        .header(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
        )
        .timeout(odds_fetch_timeout())
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !markets_resp.status().is_success() {
        return Err(format!(
            "PIN markets {} status: {}",
            sport,
            markets_resp.status()
        ));
    }

    let markets_json: Value = markets_resp.json().await.map_err(|e| e.to_string())?;
    let ts = now_ms();
    let mut out = parse_pinnacle_markets(sport, &matchups_json, &markets_json, ts)?;
    if expanded_market_scan_enabled() {
        let (related_matchups, related_markets) =
            fetch_pinnacle_guest_related(client, sport, &matchups_json).await;
        out.extend(parse_pinnacle_related_markets(
            sport,
            &matchups_json,
            &related_matchups,
            &related_markets,
            ts,
        )?);
    }
    Ok(dedup_markets(out))
}

fn parse_pinnacle_markets(
    sport: &str,
    matchups_json: &Value,
    markets_json: &Value,
    ts: u64,
) -> Result<Vec<CanonicalMarket>, String> {
    // Build matchup lookup: id → {participants, startTime, isLive}
    let mut matchup_map: std::collections::HashMap<u64, MatchupInfo> =
        std::collections::HashMap::new();
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
            let start_time = m
                .get("startTime")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
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

            matchup_map.insert(
                id,
                MatchupInfo {
                    home,
                    away,
                    home_id,
                    away_id,
                    start_time,
                    is_live,
                },
            );
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

            let mtype = mkt.get("type").and_then(|v| v.as_str()).unwrap_or("");
            let market_type = match mtype {
                "moneyline" => "moneyline",
                "spread" => "spread",
                "total" => "total",
                "team_total" => "team_total",
                _ => continue,
            };

            let period = mkt.get("period").and_then(|v| v.as_u64()).unwrap_or(0);
            if market_type == "moneyline" && period != 0 {
                continue;
            }

            // Include alternate lines for full-game (period 0) so a common line that is "main" on
            // one book and "alt" on another shares an identity bucket. Period alternates are
            // deferred until period-label parity is verified across books. marketAltId is resolved
            // by the adapter from the quote response at place time, so nothing extra is needed here.
            let is_alternate = mkt
                .get("isAlternate")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if is_alternate && period != 0 {
                continue;
            }

            let period_str = match pinnacle_period_label(period) {
                Some(label) => label,
                None => {
                    tracing::debug!(
                        "[PIN] Skipping unsupported period {} for {} market key={}",
                        period,
                        market_type,
                        mkt.get("key").and_then(|v| v.as_str()).unwrap_or("")
                    );
                    continue;
                }
            };

            let pin_event_id =
                team_names::canonical_event_id(sport, &info.away, &info.home, &info.start_time);

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

                    let designation = price
                        .get("designation")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let points = price.get("points").and_then(|v| v.as_f64());
                    let participant_id = price.get("participantId").and_then(|v| v.as_u64());

                    let raw_selection =
                        resolve_pin_selection(market_type, designation, participant_id, &info);
                    if raw_selection.is_empty() {
                        continue;
                    }
                    let selection =
                        team_names::normalize_selection(market_type, &raw_selection, sport);
                    let surface_key = if market_type == "team_total" {
                        pin_team_total_surface_key(mkt, &info, sport)
                    } else {
                        None
                    };

                    out.push(CanonicalMarket {
                        event_id: pin_event_id.clone(),
                        sport: sport.to_string(),
                        league: sport.to_string(),
                        start_time_utc: info.start_time.clone(),
                        market_type: market_type.to_string(),
                        surface_key,
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
                                "spread" => format!(
                                    "s;{};s;{}",
                                    period,
                                    points.map(|p| format!("{:.1}", p)).unwrap_or_default()
                                ),
                                "total" => format!(
                                    "s;{};ou;{}",
                                    period,
                                    points.map(|p| format!("{:.1}", p)).unwrap_or_default()
                                ),
                                "team_total" => format!(
                                    "s;{};tt;{};{}",
                                    period,
                                    points.map(|p| format!("{:.1}", p)).unwrap_or_default(),
                                    designation
                                ),
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

async fn fetch_pinnacle_guest_related(
    client: &Client,
    sport: &str,
    matchups_json: &Value,
) -> (Value, Value) {
    let mut related_matchups = Vec::new();
    let mut related_markets = Vec::new();
    let parent_ids = matchups_json
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter(|m| m.get("type").and_then(|v| v.as_str()) == Some("matchup"))
                .filter_map(|m| m.get("id").and_then(|v| v.as_u64()))
                .take(expanded_market_max_events())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    for matchup_id in parent_ids {
        let related_url = format!(
            "https://{}/0.1/matchups/{}/related",
            config::PIN_DOMAIN,
            matchup_id
        );
        match fetch_pinnacle_guest_json(client, &related_url).await {
            Ok(Value::Array(values)) => related_matchups.extend(values),
            Ok(_) => {}
            Err(e) => tracing::debug!(
                "[PIN] related matchups {} parent={} err={}",
                sport,
                matchup_id,
                e
            ),
        }

        let markets_url = format!(
            "https://{}/0.1/matchups/{}/markets/related/straight",
            config::PIN_DOMAIN,
            matchup_id
        );
        match fetch_pinnacle_guest_json(client, &markets_url).await {
            Ok(Value::Array(values)) => related_markets.extend(values),
            Ok(_) => {}
            Err(e) => tracing::debug!(
                "[PIN] related markets {} parent={} err={}",
                sport,
                matchup_id,
                e
            ),
        }
    }

    (
        Value::Array(related_matchups),
        Value::Array(related_markets),
    )
}

async fn fetch_pinnacle_guest_json(client: &Client, url: &str) -> Result<Value, String> {
    let resp = client
        .get(url)
        .header(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
        )
        .timeout(odds_fetch_timeout())
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        return Err(format!("status {}", resp.status()));
    }
    resp.json::<Value>().await.map_err(|e| e.to_string())
}

fn parse_pinnacle_related_markets(
    sport: &str,
    parent_matchups_json: &Value,
    related_matchups_json: &Value,
    related_markets_json: &Value,
    ts: u64,
) -> Result<Vec<CanonicalMarket>, String> {
    let parent_map = build_pinnacle_matchup_map(parent_matchups_json);
    let mut special_map: HashMap<u64, PinnacleSpecialInfo> = HashMap::new();

    if let Some(related_matchups) = related_matchups_json.as_array() {
        for matchup in related_matchups {
            if matchup.get("type").and_then(|v| v.as_str()) != Some("special") {
                continue;
            }
            let special = match matchup.get("special") {
                Some(special) => special,
                None => continue,
            };
            if special.get("category").and_then(|v| v.as_str()) != Some("Player Props") {
                continue;
            }
            let special_id = match matchup.get("id").and_then(|v| v.as_u64()) {
                Some(id) => id,
                None => continue,
            };
            let parent_id = match matchup.get("parentId").and_then(|v| v.as_u64()) {
                Some(id) => id,
                None => continue,
            };
            let parent = match parent_map.get(&parent_id) {
                Some(parent) => parent,
                None => continue,
            };
            let description = special
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let (player_name, prop_type) = match parse_pin_special_description(description) {
                Some(parsed) => parsed,
                None => continue,
            };

            let mut participants = HashMap::new();
            if let Some(parts) = matchup.get("participants").and_then(|v| v.as_array()) {
                for participant in parts {
                    let id = match participant.get("id").and_then(|v| v.as_u64()) {
                        Some(id) => id,
                        None => continue,
                    };
                    let name = participant
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if let Some(selection) = over_under_selection(name) {
                        participants.insert(id, selection.to_string());
                    }
                }
            }
            if participants.len() < 2 {
                continue;
            }

            special_map.insert(
                special_id,
                PinnacleSpecialInfo {
                    parent_id,
                    event_id: team_names::canonical_event_id(
                        sport,
                        &parent.away,
                        &parent.home,
                        &parent.start_time,
                    ),
                    start_time: parent.start_time.clone(),
                    is_live: parent.is_live,
                    player_name,
                    prop_type,
                    participants,
                },
            );
        }
    }

    let mut out = Vec::new();
    if let Some(markets) = related_markets_json.as_array() {
        for market in markets {
            if market.get("type").and_then(|v| v.as_str()) != Some("total") {
                continue;
            }
            let special_id = match market.get("matchupId").and_then(|v| v.as_u64()) {
                Some(id) => id,
                None => continue,
            };
            let special = match special_map.get(&special_id) {
                Some(special) => special,
                None => continue,
            };
            let market_key = market
                .get("key")
                .and_then(|v| v.as_str())
                .unwrap_or("s;0;ou");
            let period = market
                .get("period")
                .and_then(|v| v.as_u64())
                .and_then(pinnacle_period_label)
                .unwrap_or("full_game");

            if let Some(prices) = market.get("prices").and_then(|v| v.as_array()) {
                for price in prices {
                    let participant_id = match price.get("participantId").and_then(|v| v.as_u64()) {
                        Some(id) => id,
                        None => continue,
                    };
                    let selection = match special.participants.get(&participant_id) {
                        Some(selection) => selection.clone(),
                        None => continue,
                    };
                    let line = match price.get("points").and_then(|v| v.as_f64()) {
                        Some(line) => line,
                        None => continue,
                    };
                    let american_odds = match price.get("price").and_then(|v| v.as_f64()) {
                        Some(price) if price != 0.0 => price,
                        _ => continue,
                    };
                    let decimal = american_to_decimal(american_odds);
                    if decimal <= 1.0 || decimal > 100.0 {
                        continue;
                    }

                    out.push(CanonicalMarket {
                        event_id: special.event_id.clone(),
                        sport: sport.to_string(),
                        league: sport.to_string(),
                        start_time_utc: special.start_time.clone(),
                        market_type: "player_prop".to_string(),
                        surface_key: Some(player_surface_key(
                            &special.prop_type,
                            &special.player_name,
                            period,
                        )),
                        selection,
                        line: Some(line),
                        period: period.to_string(),
                        is_live: special.is_live,
                        player_name: Some(special.player_name.clone()),
                        prop_type: Some(special.prop_type.clone()),
                        decimal_odds: decimal,
                        american_odds: Some(american_odds),
                        venue: "pinnacle".to_string(),
                        venue_market_id: format!(
                            "SP:{}:{}:{}",
                            special_id, market_key, participant_id
                        ),
                        venue_event_id: format!("pin:{}:sp:{}", special.parent_id, special_id),
                        received_at_ms: ts,
                    });
                }
            }
        }
    }

    Ok(out)
}

fn build_pinnacle_matchup_map(matchups_json: &Value) -> HashMap<u64, MatchupInfo> {
    let mut matchup_map = HashMap::new();
    if let Some(arr) = matchups_json.as_array() {
        for matchup in arr {
            if matchup.get("type").and_then(|v| v.as_str()) != Some("matchup") {
                continue;
            }
            let id = match matchup.get("id").and_then(|v| v.as_u64()) {
                Some(id) => id,
                None => continue,
            };
            let start_time = matchup
                .get("startTime")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let is_live = matchup
                .get("isLive")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            let mut home = String::new();
            let mut away = String::new();
            let mut home_id = None;
            let mut away_id = None;
            if let Some(parts) = matchup.get("participants").and_then(|v| v.as_array()) {
                for participant in parts {
                    let alignment = participant
                        .get("alignment")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let name = participant
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let id = participant.get("id").and_then(|v| v.as_u64());
                    if alignment == "home" {
                        home = name.to_string();
                        home_id = id;
                    } else if alignment == "away" {
                        away = name.to_string();
                        away_id = id;
                    }
                }
            }
            matchup_map.insert(
                id,
                MatchupInfo {
                    home,
                    away,
                    home_id,
                    away_id,
                    start_time,
                    is_live,
                },
            );
        }
    }
    matchup_map
}

fn parse_pin_special_description(description: &str) -> Option<(String, String)> {
    let player = description.split('(').next()?.trim();
    if player.is_empty() {
        return None;
    }
    let prop_text = description
        .split('(')
        .nth(1)
        .and_then(|rest| rest.split(')').next())
        .unwrap_or(description);
    let prop_type = prop_type_from_text(prop_text)?;
    Some((team_names::normalize_player(player), prop_type.to_string()))
}

fn pin_team_total_surface_key(market: &Value, info: &MatchupInfo, sport: &str) -> Option<String> {
    let key = market.get("key").and_then(|v| v.as_str()).unwrap_or("");
    let side = key.split(';').last().unwrap_or("");
    let team = match side {
        "home" => &info.home,
        "away" => &info.away,
        _ => return None,
    };
    Some(team_total_surface_key(&team_names::normalize_team(
        team, sport,
    )))
}

struct PinnacleSpecialInfo {
    parent_id: u64,
    event_id: String,
    start_time: String,
    is_live: bool,
    player_name: String,
    prop_type: String,
    participants: HashMap<u64, String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn maps_supported_fd_soccer_competitions_to_league_sports() {
        assert_eq!(map_fd_soccer_competition("US MLS"), Some("MLS"));
        assert_eq!(
            map_fd_soccer_competition("English Premier League"),
            Some("EPL")
        );
        assert_eq!(
            map_fd_soccer_competition("UEFA Champions League"),
            Some("UCL")
        );
        assert_eq!(map_fd_soccer_competition("Premier League"), Some("EPL"));
        assert_eq!(map_fd_soccer_competition("German Bundesliga 2"), None);
    }

    #[test]
    fn fd_soccer_parser_uses_competition_league_for_event_id() {
        let payload = json!({
            "attachments": {
                "competitions": {
                    "141": { "competitionId": 141, "name": "US MLS" }
                },
                "events": {
                    "100": {
                        "eventId": 100,
                        "name": "Toronto FC v New York City FC",
                        "openDate": "2026-05-10T23:00:00.000Z",
                        "competitionId": 141,
                        "inPlay": false
                    }
                },
                "markets": {
                    "200": {
                        "eventId": 100,
                        "marketId": 200,
                        "marketName": "Moneyline",
                        "marketType": "MONEY_LINE",
                        "runners": [
                            {
                                "runnerName": "Toronto FC",
                                "selectionId": 1,
                                "winRunnerOdds": {
                                    "trueOdds": { "decimalOdds": { "decimalOdds": 2.1 } },
                                    "americanDisplayOdds": { "americanOdds": 110 }
                                }
                            },
                            {
                                "runnerName": "New York City FC",
                                "selectionId": 2,
                                "winRunnerOdds": {
                                    "trueOdds": { "decimalOdds": { "decimalOdds": 3.4 } },
                                    "americanDisplayOdds": { "americanOdds": 240 }
                                }
                            }
                        ]
                    }
                }
            }
        });

        let markets = parse_fd_response(&payload, "SOCCER").expect("parse fd soccer");
        assert_eq!(markets.len(), 2);
        assert!(markets.iter().all(|m| m.sport == "MLS"));
        assert!(markets.iter().all(|m| m.event_id.starts_with("MLS:")));
        assert!(markets.iter().all(|m| m.venue_event_id == "fd:100"));
    }

    #[test]
    fn fd_soccer_parser_skips_unsupported_competitions() {
        let payload = json!({
            "attachments": {
                "competitions": {
                    "61": { "competitionId": 61, "name": "German Bundesliga 2" }
                },
                "events": {
                    "100": {
                        "eventId": 100,
                        "name": "Team A v Team B",
                        "openDate": "2026-05-10T23:00:00.000Z",
                        "competitionId": 61,
                        "inPlay": false
                    }
                },
                "markets": {
                    "200": {
                        "eventId": 100,
                        "marketId": 200,
                        "marketName": "Moneyline",
                        "marketType": "MONEY_LINE",
                        "runners": [
                            {
                                "runnerName": "Team A",
                                "selectionId": 1,
                                "winRunnerOdds": {
                                    "trueOdds": { "decimalOdds": { "decimalOdds": 2.1 } }
                                }
                            },
                            {
                                "runnerName": "Team B",
                                "selectionId": 2,
                                "winRunnerOdds": {
                                    "trueOdds": { "decimalOdds": { "decimalOdds": 3.4 } }
                                }
                            }
                        ]
                    }
                }
            }
        });

        let markets = parse_fd_response(&payload, "SOCCER").expect("parse fd soccer");
        assert!(markets.is_empty());
    }

    #[test]
    fn dk_event_subcategory_parses_player_prop_over_under() {
        let payload = json!({
            "markets": [{
                "id": "340023994",
                "name": "Jalen Brunson Assists O/U",
                "marketType": { "name": "Player Assists O/U" }
            }],
            "selections": [
                {
                    "id": "over-sel",
                    "marketId": "340023994",
                    "label": "Over",
                    "points": 6.5,
                    "trueOdds": 1.91,
                    "displayOdds": { "american": "-110", "decimal": "1.91" },
                    "participants": [{ "name": "Jalen Brunson", "type": "Player" }]
                },
                {
                    "id": "under-sel",
                    "marketId": "340023994",
                    "label": "Under",
                    "points": 6.5,
                    "trueOdds": 1.91,
                    "displayOdds": { "american": "-110", "decimal": "1.91" },
                    "participants": [{ "name": "Jalen Brunson", "type": "Player" }]
                }
            ]
        });
        let event = DkEventContext {
            event_id: "34226734".to_string(),
            canonical_event_id: "NBA:2026-06-06T00:NYK-vs-SAS".to_string(),
            start_time: "2026-06-06T00:30:00Z".to_string(),
            is_live: false,
        };

        let markets = parse_dk_event_subcategory_response(&payload, "NBA", &event, 1);
        assert_eq!(markets.len(), 2);
        assert!(markets.iter().all(|m| m.market_type == "player_prop"));
        assert!(markets
            .iter()
            .all(|m| m.player_name.as_deref() == Some("jalen brunson")));
        assert!(markets
            .iter()
            .all(|m| m.prop_type.as_deref() == Some("assists")));
        assert!(markets.iter().all(|m| m.line == Some(6.5)));
        assert!(markets.iter().all(|m| {
            m.surface_key.as_deref() == Some("player:assists:jalen brunson:full_game")
        }));
    }

    #[test]
    fn fd_event_page_parses_alt_player_prop_line_from_runner_name() {
        let payload = json!({
            "attachments": {
                "events": {
                    "35669206": {
                        "eventId": 35669206,
                        "name": "NY Knicks @ SA Spurs",
                        "openDate": "2026-06-06T00:30:00.000Z",
                        "inPlay": false
                    }
                },
                "markets": {
                    "801.171663780": {
                        "eventId": 35669206,
                        "marketId": "801.171663780",
                        "marketName": "Jalen Brunson - Alt Assists",
                        "marketType": "PLAYER_A_ALT_TOTAL_ASSISTS",
                        "runners": [
                            {
                                "runnerName": "Jalen Brunson Over 6.5",
                                "selectionId": 23237967,
                                "handicap": 0,
                                "winRunnerOdds": {
                                    "trueOdds": { "decimalOdds": { "decimalOdds": 1.95 } },
                                    "americanDisplayOdds": { "americanOdds": -105 }
                                }
                            },
                            {
                                "runnerName": "Jalen Brunson Under 6.5",
                                "selectionId": 23237968,
                                "handicap": 0,
                                "winRunnerOdds": {
                                    "trueOdds": { "decimalOdds": { "decimalOdds": 1.87 } },
                                    "americanDisplayOdds": { "americanOdds": -115 }
                                }
                            }
                        ]
                    }
                }
            }
        });

        let markets = parse_fd_response(&payload, "NBA").expect("parse fd event page");
        assert_eq!(markets.len(), 2);
        assert!(markets.iter().all(|m| m.market_type == "player_prop"));
        assert!(markets
            .iter()
            .all(|m| m.player_name.as_deref() == Some("jalen brunson")));
        assert!(markets
            .iter()
            .all(|m| m.prop_type.as_deref() == Some("assists")));
        assert!(markets.iter().all(|m| m.line == Some(6.5)));
    }

    #[test]
    fn pinnacle_related_parser_emits_special_player_prop() {
        let parent_matchups = json!([{
            "type": "matchup",
            "id": 1631470750,
            "startTime": "2026-06-04T17:05:00Z",
            "isLive": false,
            "participants": [
                { "alignment": "home", "name": "Philadelphia Phillies" },
                { "alignment": "away", "name": "San Diego Padres" }
            ]
        }]);
        let related_matchups = json!([{
            "id": 1631664241,
            "type": "special",
            "parentId": 1631470750,
            "special": {
                "category": "Player Props",
                "description": "Zack Wheeler (Pitching Outs)(must start)"
            },
            "participants": [
                { "id": 1631664242, "name": "Over" },
                { "id": 1631664243, "name": "Under" }
            ]
        }]);
        let related_markets = json!([{
            "matchupId": 1631664241,
            "type": "total",
            "key": "s;0;ou",
            "period": 0,
            "prices": [
                { "participantId": 1631664242, "points": 18.5, "price": 143 },
                { "participantId": 1631664243, "points": 18.5, "price": -193 }
            ]
        }]);

        let markets = parse_pinnacle_related_markets(
            "MLB",
            &parent_matchups,
            &related_matchups,
            &related_markets,
            1,
        )
        .expect("parse pinnacle related");
        assert_eq!(markets.len(), 2);
        assert!(markets.iter().all(|m| m.market_type == "player_prop"));
        assert!(markets
            .iter()
            .all(|m| m.player_name.as_deref() == Some("zack wheeler")));
        assert!(markets
            .iter()
            .all(|m| m.prop_type.as_deref() == Some("pitching_outs")));
        assert!(markets
            .iter()
            .all(|m| m.venue_market_id.starts_with("SP:1631664241:")));
    }

    #[test]
    fn pinnacle_parser_skips_non_full_game_moneylines() {
        let matchups = json!([{
            "type": "matchup",
            "id": 1630404899,
            "startTime": "2026-05-09T20:00:00Z",
            "isLive": true,
            "participants": [
                { "id": 10, "alignment": "home", "name": "Cincinnati Reds" },
                { "id": 20, "alignment": "away", "name": "Houston Astros" }
            ]
        }]);
        let markets = json!([
            {
                "matchupId": 1630404899,
                "type": "moneyline",
                "period": 10,
                "key": "s;10;m",
                "prices": [
                    { "designation": "home", "participantId": 10, "price": 250 },
                    { "designation": "away", "participantId": 20, "price": 290 }
                ]
            },
            {
                "matchupId": 1630404899,
                "type": "moneyline",
                "period": 0,
                "key": "s;0;m",
                "prices": [
                    { "designation": "home", "participantId": 10, "price": -120 },
                    { "designation": "away", "participantId": 20, "price": 110 }
                ]
            }
        ]);

        let parsed =
            parse_pinnacle_markets("MLB", &matchups, &markets, 1).expect("parse pinnacle markets");
        assert_eq!(parsed.len(), 2);
        assert!(parsed.iter().all(|m| m.period == "full_game"));
        assert!(parsed.iter().all(|m| m.venue_market_id.contains(":s;0;m:")));
    }

    #[test]
    fn pinnacle_parser_skips_unknown_periods_instead_of_full_game_fallback() {
        let matchups = json!([{
            "type": "matchup",
            "id": 1630404375,
            "startTime": "2026-05-09T20:00:00Z",
            "isLive": true,
            "participants": [
                { "id": 10, "alignment": "home", "name": "Baltimore Orioles" },
                { "id": 20, "alignment": "away", "name": "Athletics" }
            ]
        }]);
        let markets = json!([{
            "matchupId": 1630404375,
            "type": "total",
            "period": 10,
            "key": "s;10;ou;0.5",
            "prices": [
                { "designation": "over", "points": 0.5, "price": 105 },
                { "designation": "under", "points": 0.5, "price": -115 }
            ]
        }]);

        let parsed =
            parse_pinnacle_markets("MLB", &matchups, &markets, 1).expect("parse pinnacle markets");
        assert!(parsed.is_empty());
    }
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
        "total" | "team_total" => match designation {
            "over" => "over".to_string(),
            "under" => "under".to_string(),
            _ => String::new(),
        },
        _ => String::new(),
    }
}

// Selection normalization moved to team_names::normalize_selection()
