pub struct SportConfig {
    pub name: &'static str,
    pub dk_id: Option<u64>,
    pub fd_page: Option<&'static str>,
    pub fd_event_type: Option<u64>,
    pub pin_league_ids: Vec<u64>,
}

pub fn sports() -> Vec<SportConfig> {
    vec![
        // ── American Sports ─────────────────────────────────────────
        SportConfig {
            name: "NBA",
            dk_id: Some(42648),
            fd_page: Some("nba"),
            fd_event_type: None,
            pin_league_ids: vec![487],
        },
        SportConfig {
            name: "NHL",
            dk_id: Some(42133),
            fd_page: Some("nhl"),
            fd_event_type: None,
            pin_league_ids: vec![1456],
        },
        SportConfig {
            name: "MLB",
            dk_id: Some(84240),
            fd_page: Some("mlb"),
            fd_event_type: None,
            pin_league_ids: vec![246],
        },
        SportConfig {
            name: "NFL",
            dk_id: Some(88808),
            fd_page: Some("nfl"),
            fd_event_type: None,
            pin_league_ids: vec![], // PIN league 889 returns 401 consistently
        },
        SportConfig {
            name: "NCAAB",
            dk_id: Some(92483),
            fd_page: Some("ncaab"),
            fd_event_type: None,
            pin_league_ids: vec![493],
        },
        SportConfig {
            name: "NCAAF",
            dk_id: Some(87637),
            fd_page: Some("ncaaf"),
            fd_event_type: None,
            pin_league_ids: vec![880],
        },

        // ── Soccer ──────────────────────────────────────────────────
        // FD uses eventTypeId=1 for ALL soccer — only fetch once via SOCCER config.
        // DK and PIN have per-league endpoints, so each league has its own entry.
        SportConfig {
            name: "SOCCER",
            dk_id: None, // DK soccer handled per-league below
            fd_page: None,
            fd_event_type: Some(1), // Single FD fetch for all soccer
            pin_league_ids: vec![], // PIN soccer handled per-league below
        },
        SportConfig {
            name: "EPL",
            dk_id: Some(40253),
            fd_page: None,
            fd_event_type: None, // Already fetched via SOCCER
            pin_league_ids: vec![1980],
        },
        SportConfig {
            name: "LA_LIGA",
            dk_id: Some(40031),
            fd_page: None,
            fd_event_type: None,
            pin_league_ids: vec![2196],
        },
        SportConfig {
            name: "SERIE_A",
            dk_id: Some(40030),
            fd_page: None,
            fd_event_type: None,
            pin_league_ids: vec![2436],
        },
        SportConfig {
            name: "BUNDESLIGA",
            dk_id: Some(40481),
            fd_page: None,
            fd_event_type: None,
            pin_league_ids: vec![1842],
        },
        SportConfig {
            name: "LIGUE_1",
            dk_id: Some(40032),
            fd_page: None,
            fd_event_type: None,
            pin_league_ids: vec![2036],
        },
        SportConfig {
            name: "UCL",
            dk_id: Some(40685),
            fd_page: None,
            fd_event_type: None,
            pin_league_ids: vec![2627],
        },
        SportConfig {
            name: "MLS",
            dk_id: Some(89345),
            fd_page: None,
            fd_event_type: None, // Already fetched via SOCCER
            pin_league_ids: vec![2663],
        },

        // ── Combat Sports ───────────────────────────────────────────
        SportConfig {
            name: "UFC",
            dk_id: Some(9034),
            fd_page: None,
            fd_event_type: None,
            pin_league_ids: vec![1624],
        },
    ]
}

pub const DK_HOSTNAME: &str = "sportsbook-nash.draftkings.com";
pub const DK_SITE_CODE: &str = "CA-ON-SB";

pub const FD_STATES: &[&str] = &["nj", "mi"];
pub const FD_API_KEY: &str = "FhMFpcPWXMeyZxOx";

pub const PIN_DOMAIN: &str = "guest.api.arcadia.pinnacle.com";

pub const POLL_INTERVAL_SECS: u64 = 5; // Arbs are fleeting — poll fast
pub const MAX_DATA_AGE_MS: u64 = 30_000; // Tighter: 30s data freshness

pub const DEFAULT_INITIAL_BALANCE: f64 = 1000.0;
pub const VENUES: &[&str] = &["draftkings", "fanduel", "pinnacle"];
