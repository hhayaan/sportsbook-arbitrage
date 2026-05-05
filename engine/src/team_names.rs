use std::collections::HashMap;
use std::sync::LazyLock;

/// Canonical team code lookup. Maps all known name variants → 3-letter code.
/// Key: lowercase full name, nickname, abbreviation, or alternate spelling.
/// Value: canonical 3-letter uppercase code.
static TEAM_MAP: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    let entries: Vec<(&str, &[&str])> = vec![
        // ── NBA ────────────────────────────────────────────────────────
        ("ATL", &["atlanta hawks", "hawks", "atlanta", "atl"]),
        ("BOS", &["boston celtics", "celtics", "boston", "bos"]),
        ("BKN", &["brooklyn nets", "nets", "brooklyn", "bkn", "bk nets"]),
        ("CHA", &["charlotte hornets", "hornets", "charlotte", "cha"]),
        ("CHI", &["chicago bulls", "bulls", "chicago", "chi"]),
        ("CLE", &["cleveland cavaliers", "cavaliers", "cavs", "cleveland", "cle"]),
        ("DAL", &["dallas mavericks", "mavericks", "mavs", "dallas", "dal"]),
        ("DEN", &["denver nuggets", "nuggets", "denver", "den"]),
        ("DET", &["detroit pistons", "pistons", "detroit", "det"]),
        ("GSW", &["golden state warriors", "warriors", "golden state", "gs warriors", "gsw", "gs"]),
        ("HOU", &["houston rockets", "rockets", "houston", "hou"]),
        ("IND", &["indiana pacers", "pacers", "indiana", "ind"]),
        ("LAC", &["los angeles clippers", "la clippers", "clippers", "lac"]),
        ("LAL", &["los angeles lakers", "la lakers", "lakers", "lal"]),
        ("MEM", &["memphis grizzlies", "grizzlies", "memphis", "mem"]),
        ("MIA", &["miami heat", "heat", "miami", "mia"]),
        ("MIL", &["milwaukee bucks", "bucks", "milwaukee", "mil"]),
        ("MIN", &["minnesota timberwolves", "timberwolves", "wolves", "minnesota", "min"]),
        ("NOP", &["new orleans pelicans", "pelicans", "new orleans", "nop", "no pelicans"]),
        ("NYK", &["new york knicks", "knicks", "ny knicks", "nyk"]),
        ("OKC", &["oklahoma city thunder", "thunder", "okc", "oklahoma city", "ok thunder"]),
        ("ORL", &["orlando magic", "magic", "orlando", "orl"]),
        ("PHI", &["philadelphia 76ers", "76ers", "sixers", "philadelphia", "phi", "philly"]),
        ("PHX", &["phoenix suns", "suns", "phoenix", "phx"]),
        ("POR", &["portland trail blazers", "trail blazers", "blazers", "portland", "por"]),
        ("SAC", &["sacramento kings", "kings", "sacramento", "sac"]),
        ("SAS", &["san antonio spurs", "spurs", "san antonio", "sas", "sa spurs"]),
        ("TOR", &["toronto raptors", "raptors", "toronto", "tor"]),
        ("UTA", &["utah jazz", "jazz", "utah", "uta"]),
        ("WAS", &["washington wizards", "wizards", "washington", "was"]),

        // ── NFL ────────────────────────────────────────────────────────
        ("ARI", &["arizona cardinals", "cardinals", "arizona", "ari", "az cardinals"]),
        ("BAL", &["baltimore ravens", "ravens", "baltimore", "bal"]),
        ("BUF", &["buffalo bills", "bills", "buffalo", "buf"]),
        ("CAR", &["carolina panthers", "panthers", "carolina", "car"]),
        ("CIN", &["cincinnati bengals", "bengals", "cincinnati", "cin"]),
        ("GB",  &["green bay packers", "packers", "green bay", "gb"]),
        ("JAX", &["jacksonville jaguars", "jaguars", "jacksonville", "jax"]),
        ("KC",  &["kansas city chiefs", "chiefs", "kansas city", "kc"]),
        ("LV",  &["las vegas raiders", "raiders", "las vegas", "lv", "oakland raiders"]),
        ("LAR", &["los angeles rams", "la rams", "rams", "lar"]),
        ("NE",  &["new england patriots", "patriots", "new england", "ne", "pats"]),
        ("NO",  &["new orleans saints", "saints", "no saints"]),
        ("NYG", &["new york giants", "giants", "ny giants", "nyg"]),
        ("NYJ", &["new york jets", "jets", "ny jets", "nyj"]),
        ("PIT", &["pittsburgh steelers", "steelers", "pittsburgh", "pit"]),
        ("SF",  &["san francisco 49ers", "49ers", "niners", "san francisco", "sf"]),
        ("SEA", &["seattle seahawks", "seahawks", "seattle", "sea"]),
        ("TB",  &["tampa bay buccaneers", "buccaneers", "bucs", "tampa bay", "tb"]),
        ("TEN", &["tennessee titans", "titans", "tennessee", "ten"]),

        // ── NHL ────────────────────────────────────────────────────────
        ("ANA", &["anaheim ducks", "ducks", "anaheim", "ana"]),
        ("BOS-NHL", &["boston bruins", "bruins"]),
        ("BUF-NHL", &["buffalo sabres", "sabres"]),
        ("CGY", &["calgary flames", "flames", "calgary", "cgy"]),
        ("CAR-NHL", &["carolina hurricanes", "hurricanes", "canes"]),
        ("CHI-NHL", &["chicago blackhawks", "blackhawks", "hawks"]),
        ("COL", &["colorado avalanche", "avalanche", "avs", "colorado", "col"]),
        ("CBJ", &["columbus blue jackets", "blue jackets", "columbus", "cbj"]),
        ("DAL-NHL", &["dallas stars", "stars"]),
        ("DET-NHL", &["detroit red wings", "red wings"]),
        ("EDM", &["edmonton oilers", "oilers", "edmonton", "edm"]),
        ("FLA", &["florida panthers", "fla panthers", "florida", "fla"]),
        ("LAK", &["los angeles kings", "la kings", "lak"]),
        ("MIN-NHL", &["minnesota wild", "wild"]),
        ("MTL", &["montreal canadiens", "canadiens", "habs", "montreal", "mtl"]),
        ("NSH", &["nashville predators", "predators", "preds", "nashville", "nsh"]),
        ("NJD", &["new jersey devils", "devils", "nj devils", "njd"]),
        ("NYI", &["new york islanders", "islanders", "ny islanders", "nyi"]),
        ("NYR", &["new york rangers", "rangers", "ny rangers", "nyr"]),
        ("OTT", &["ottawa senators", "senators", "sens", "ottawa", "ott"]),
        ("PHI-NHL", &["philadelphia flyers", "flyers"]),
        ("PIT-NHL", &["pittsburgh penguins", "penguins", "pens"]),
        ("SJS", &["san jose sharks", "sharks", "san jose", "sjs"]),
        ("STL", &["st louis blues", "blues", "st. louis blues", "stl"]),
        ("TBL", &["tampa bay lightning", "lightning", "tbl"]),
        ("TOR-NHL", &["toronto maple leafs", "maple leafs", "leafs"]),
        ("VAN", &["vancouver canucks", "canucks", "vancouver", "van"]),
        ("VGK", &["vegas golden knights", "golden knights", "vegas", "vgk"]),
        ("WPG", &["winnipeg jets", "wpg jets", "winnipeg", "wpg"]),
        ("WSH", &["washington capitals", "capitals", "caps", "wsh"]),

        // ── MLB ────────────────────────────────────────────────────────
        ("ARI-MLB", &["arizona diamondbacks", "diamondbacks", "dbacks", "d-backs"]),
        ("ATL-MLB", &["atlanta braves", "braves"]),
        ("BAL-MLB", &["baltimore orioles", "orioles"]),
        ("BOS-MLB", &["boston red sox", "red sox"]),
        ("CHC", &["chicago cubs", "cubs", "chc"]),
        ("CWS", &["chicago white sox", "white sox", "cws"]),
        ("CIN-MLB", &["cincinnati reds", "reds"]),
        ("CLE-MLB", &["cleveland guardians", "guardians"]),
        ("COL-MLB", &["colorado rockies", "rockies"]),
        ("DET-MLB", &["detroit tigers", "tigers"]),
        ("HOU-MLB", &["houston astros", "astros"]),
        ("KC-MLB", &["kansas city royals", "royals"]),
        ("LAA", &["los angeles angels", "la angels", "angels", "laa"]),
        ("LAD", &["los angeles dodgers", "la dodgers", "dodgers", "lad"]),
        ("MIA-MLB", &["miami marlins", "marlins"]),
        ("MIL-MLB", &["milwaukee brewers", "brewers"]),
        ("MIN-MLB", &["minnesota twins", "twins"]),
        ("NYM", &["new york mets", "mets", "ny mets", "nym"]),
        ("NYY", &["new york yankees", "yankees", "ny yankees", "nyy"]),
        ("OAK", &["oakland athletics", "athletics", "oakland", "oak", "a's"]),
        ("PHI-MLB", &["philadelphia phillies", "phillies"]),
        ("PIT-MLB", &["pittsburgh pirates", "pirates"]),
        ("SD",  &["san diego padres", "padres", "san diego", "sd"]),
        ("SF-MLB", &["san francisco giants", "sf giants"]),
        ("SEA-MLB", &["seattle mariners", "mariners"]),
        ("STL-MLB", &["st louis cardinals", "st. louis cardinals", "stl cardinals"]),
        ("TB-MLB", &["tampa bay rays", "rays"]),
        ("TEX", &["texas rangers", "tex rangers", "texas", "tex"]),
        ("TOR-MLB", &["toronto blue jays", "blue jays"]),
        ("WSH-MLB", &["washington nationals", "nationals", "nats"]),

        // ── Soccer (EPL + MLS) ─────────────────────────────────────────
        ("ARS", &["arsenal", "arsenal fc", "ars"]),
        ("AVL", &["aston villa", "villa", "avl"]),
        ("BOU", &["afc bournemouth", "bournemouth", "bou"]),
        ("BRE", &["brentford", "brentford fc", "bre"]),
        ("BHA", &["brighton", "brighton & hove albion", "brighton and hove albion", "bha"]),
        ("CHE", &["chelsea", "chelsea fc", "che"]),
        ("CRY", &["crystal palace", "palace", "cry"]),
        ("EVE", &["everton", "everton fc", "eve"]),
        ("FUL", &["fulham", "fulham fc", "ful"]),
        ("IPS", &["ipswich", "ipswich town", "ips"]),
        ("LEI", &["leicester", "leicester city", "lei"]),
        ("LIV", &["liverpool", "liverpool fc", "liv"]),
        ("MCI", &["manchester city", "man city", "mci"]),
        ("MUN", &["manchester united", "man united", "man utd", "mun"]),
        ("NEW", &["newcastle", "newcastle united", "new"]),
        ("NFO", &["nottingham forest", "nott forest", "nfo"]),
        ("SOU", &["southampton", "saints fc", "sou"]),
        ("TOT", &["tottenham", "tottenham hotspur", "spurs fc", "tot"]),
        ("WHU", &["west ham", "west ham united", "whu"]),
        ("WOL", &["wolverhampton", "wolves fc", "wolverhampton wanderers", "wol"]),

        // ── NCAAB (Power Conference + Top Programs) ───────────────────
        ("DUKE", &["duke blue devils", "duke", "blue devils"]),
        ("UNC",  &["north carolina tar heels", "north carolina", "tar heels", "unc"]),
        ("UK",   &["kentucky wildcats", "kentucky", "uk wildcats"]),
        ("KU",   &["kansas jayhawks", "kansas", "jayhawks", "ku"]),
        ("BAMA", &["alabama crimson tide", "alabama", "crimson tide", "bama"]),
        ("ARIZ", &["arizona wildcats", "ariz wildcats"]),
        ("ARK",  &["arkansas razorbacks", "arkansas", "razorbacks", "ark"]),
        ("AUB",  &["auburn tigers", "auburn", "aub"]),
        ("BAY",  &["baylor bears", "baylor", "bears", "bay"]),
        ("CONN", &["uconn huskies", "uconn", "connecticut", "conn"]),
        ("CREIG", &["creighton bluejays", "creighton", "bluejays"]),
        ("FLA-NCAAB", &["florida gators", "gators"]),
        ("GONZ", &["gonzaga bulldogs", "gonzaga", "zags", "gonz"]),
        ("HOU-NCAAB", &["houston cougars", "cougars"]),
        ("ILL",  &["illinois fighting illini", "illinois", "illini", "ill"]),
        ("IOWA", &["iowa hawkeyes", "iowa", "hawkeyes"]),
        ("ISU",  &["iowa state cyclones", "iowa state", "cyclones", "isu"]),
        ("IU",   &["indiana hoosiers", "hoosiers", "iu"]),
        ("LSU",  &["lsu tigers", "lsu"]),
        ("MARQ", &["marquette golden eagles", "marquette"]),
        ("MD",   &["maryland terrapins", "maryland", "terps", "md"]),
        ("MICH", &["michigan wolverines", "michigan", "wolverines", "mich"]),
        ("MSU",  &["michigan state spartans", "michigan state", "spartans", "msu"]),
        ("NCST", &["nc state wolfpack", "nc state", "wolfpack", "ncst"]),
        ("ND",   &["notre dame fighting irish", "notre dame", "fighting irish", "nd"]),
        ("OSU",  &["ohio state buckeyes", "ohio state", "buckeyes", "osu"]),
        ("ORE",  &["oregon ducks", "oregon", "ore"]),
        ("PSU",  &["penn state nittany lions", "penn state", "nittany lions", "psu"]),
        ("PUR",  &["purdue boilermakers", "purdue", "boilermakers", "pur"]),
        ("RUTG", &["rutgers scarlet knights", "rutgers", "scarlet knights"]),
        ("STAN", &["stanford cardinal", "stanford", "stan"]),
        ("SYR",  &["syracuse orange", "syracuse", "syr"]),
        ("TENN", &["tennessee volunteers", "volunteers", "vols"]),
        ("TXAM", &["texas a&m aggies", "texas a&m", "aggies", "tamu", "txam"]),
        ("TEX-NCAAB", &["texas longhorns", "longhorns"]),
        ("TTU",  &["texas tech red raiders", "texas tech", "red raiders", "ttu"]),
        ("UCLA", &["ucla bruins", "ucla"]),
        ("UVA",  &["virginia cavaliers", "virginia", "uva"]),
        ("VILL", &["villanova wildcats", "villanova", "vill"]),
        ("VT",   &["virginia tech hokies", "virginia tech", "hokies", "vt"]),
        ("WAKE", &["wake forest demon deacons", "wake forest", "demon deacons", "wake"]),
        ("WISC", &["wisconsin badgers", "wisconsin", "badgers", "wisc"]),
        ("WVU",  &["west virginia mountaineers", "west virginia", "mountaineers", "wvu"]),
        ("XAVI", &["xavier musketeers", "xavier", "xavi"]),

        // ── NCAAF (same schools, different sport context) ──────────────
        // Most NCAAF teams share NCAAB codes — the sport prefix in event_id disambiguates

        // ── UFC (use fighter last names as codes) ──────────────────────
        // UFC fighters don't have stable team codes — handled by surname matching below
    ];

    let mut map = HashMap::new();
    for (code, aliases) in entries {
        for alias in aliases {
            map.insert(*alias, code);
        }
    }
    map
});

/// Normalize a team/fighter name to its canonical code.
/// Uses a 10-step fallback pipeline (ported from V2's team-names.ts).
pub fn normalize_team(raw: &str, _sport: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return "UNKNOWN".to_string();
    }

    let cleaned = trimmed.to_lowercase()
        .replace(".", "")
        .replace("'", "")
        .replace("'", "");
    let cleaned = cleaned.trim();

    // Step 1: Direct exact lookup
    if let Some(code) = TEAM_MAP.get(cleaned) {
        return strip_sport_suffix(code).to_string();
    }

    // Step 2: Check if input is already a valid 2-4 char code (e.g., "DET", "BOS")
    if trimmed.len() <= 4 && trimmed.chars().all(|c| c.is_ascii_uppercase()) {
        if TEAM_MAP.values().any(|&code| strip_sport_suffix(code) == trimmed) {
            return trimmed.to_string();
        }
    }

    // Step 3: Case-insensitive already handled by step 1 (we lowercased)

    // Step 4: Last word match ("Det Pistons" → lookup "pistons" → "DET")
    let words: Vec<&str> = cleaned.split_whitespace().collect();
    if words.len() >= 2 {
        if let Some(code) = TEAM_MAP.get(words[words.len() - 1]) {
            return strip_sport_suffix(code).to_string();
        }
    }

    // Step 5: First word match ("Stanford Cardinal" → lookup "stanford")
    if words.len() >= 2 {
        if let Some(code) = TEAM_MAP.get(words[0]) {
            return strip_sport_suffix(code).to_string();
        }
    }

    // Step 6: Strip parentheses/brackets for FD format like "(1) Stanford" or "(OH) Ohio State"
    let stripped = cleaned
        .replace(|c: char| c == '(' || c == ')' || c == '[' || c == ']', "")
        .trim()
        .to_string();
    let stripped = stripped.trim_start_matches(|c: char| c.is_numeric() || c == ' ');
    let stripped = stripped.trim();

    if stripped != cleaned {
        if let Some(code) = TEAM_MAP.get(stripped) {
            return strip_sport_suffix(code).to_string();
        }

        // Step 7: Cleaned last word after bracket strip
        let stripped_words: Vec<&str> = stripped.split_whitespace().collect();
        if stripped_words.len() >= 2 {
            if let Some(code) = TEAM_MAP.get(stripped_words[stripped_words.len() - 1]) {
                return strip_sport_suffix(code).to_string();
            }
        }
    }

    // Step 8: Two-part code generation (first 3 chars of first word + first 3 chars of last word)
    if words.len() >= 2 {
        let first = &words[0];
        let last = &words[words.len() - 1];
        let code = format!(
            "{}{}",
            first.chars().take(3).collect::<String>().to_uppercase(),
            last.chars().take(3).collect::<String>().to_uppercase(),
        );
        return code;
    }

    // Step 9: Single word — up to 4 uppercase chars
    if words.len() == 1 {
        return cleaned.chars().take(4).collect::<String>().to_uppercase();
    }

    // Step 10: Final fallback — original uppercased
    trimmed.replace(' ', "_").to_uppercase()
}

/// Strip sport suffixes like "-NHL", "-MLB" from codes.
/// Since the event_id already includes the sport prefix (e.g., NHL:2026-03-15:BOS-vs-NJD),
/// the base code is sufficient for uniqueness within that sport context.
fn strip_sport_suffix(code: &str) -> &str {
    if let Some(pos) = code.find('-') {
        &code[..pos]
    } else {
        code
    }
}

/// Build a canonical event ID from two team names and sport.
/// Format: `{SPORT}:{DATE}:{AWAY}-vs-{HOME}`
/// If date can't be extracted, uses teams only: `{SPORT}:{AWAY}-vs-{HOME}`
pub fn canonical_event_id(sport: &str, away: &str, home: &str, start_time: &str) -> String {
    let away_code = normalize_team(away, sport);
    let home_code = normalize_team(home, sport);

    // ALWAYS sort team codes alphabetically so "BOS-vs-NYK" == "NYK-vs-BOS"
    let (team_a, team_b) = if away_code <= home_code {
        (away_code, home_code)
    } else {
        (home_code, away_code)
    };

    let date = extract_date(start_time);

    match date {
        Some(d) => format!("{}:{}:{}-vs-{}", sport, d, team_a, team_b),
        None => {
            // Hash fallback to prevent different games from colliding
            let hash: String = start_time.chars()
                .filter(|c| c.is_alphanumeric())
                .take(12)
                .collect();
            format!("{}:{}:{}-vs-{}", sport, hash, team_a, team_b)
        }
    }
}

/// Normalize a selection name for cross-venue matching.
/// For moneyline/spread: returns the team code.
/// For total/team_total: returns "over" or "under".
pub fn normalize_selection(market_type: &str, raw: &str, sport: &str) -> String {
    let lower = raw.trim().to_lowercase();
    match market_type {
        "total" | "team_total" => {
            if lower.starts_with("over") || lower == "o" {
                "over".to_string()
            } else if lower.starts_with("under") || lower == "u" {
                "under".to_string()
            } else {
                normalize_team(raw, sport)
            }
        }
        _ => {
            let lower = raw.trim().to_lowercase();
            if lower == "draw" || lower == "the draw" || lower == "tie" {
                return "draw".to_string();
            }
            normalize_team(raw, sport)
        }
    }
}

fn extract_date(s: &str) -> Option<String> {
    // ISO 8601 with hour: "2026-03-15T19:30:00Z" → "2026-03-15T19"
    // Include hour to disambiguate doubleheader G1/G2 (same teams, same day, different times)
    if s.len() >= 13 && s.as_bytes().get(4) == Some(&b'-') && s.as_bytes().get(7) == Some(&b'-') && s.as_bytes().get(10) == Some(&b'T') {
        return Some(s[..13].to_string());
    }
    // Fallback: date only
    if s.len() >= 10 && s.as_bytes().get(4) == Some(&b'-') && s.as_bytes().get(7) == Some(&b'-') {
        return Some(s[..10].to_string());
    }
    // Epoch milliseconds: "1711756200000" → date with hour
    if let Ok(epoch_ms) = s.parse::<i64>() {
        let secs = if epoch_ms > 1_000_000_000_000 { epoch_ms / 1000 } else { epoch_ms };
        if let Some(dt) = chrono::DateTime::from_timestamp(secs, 0) {
            return Some(dt.format("%Y-%m-%dT%H").to_string());
        }
    }
    None
}

/// Normalize a player name for cross-venue matching.
/// "Luka Doncic" / "L. Doncic" / "Doncic, Luka" → "doncic"
pub fn normalize_player(raw: &str) -> String {
    let cleaned = raw.trim().to_lowercase()
        .replace(".", "")
        .replace("'", "")
        .replace("'", "")
        .replace("-", " ");

    if cleaned.contains(',') {
        let parts: Vec<&str> = cleaned.splitn(2, ',').collect();
        return parts[0].trim().to_string();
    }

    let words: Vec<&str> = cleaned.split_whitespace().collect();
    if words.len() >= 2 {
        return words[words.len() - 1].to_string();
    }

    cleaned
}

/// Extract prop type from market name.
/// "Luka Doncic Total Points" → ("doncic", "points")
pub fn parse_player_prop(market_name: &str) -> Option<(String, String)> {
    let lower = market_name.to_lowercase();

    let prop_type = if lower.contains("points") && lower.contains("reb") && lower.contains("ast") {
        "pra"
    } else if lower.contains("points") && lower.contains("reb") {
        "pr"
    } else if lower.contains("points") && lower.contains("ast") {
        "pa"
    } else if lower.contains("reb") && lower.contains("ast") {
        "ra"
    } else if lower.contains("3 point") || lower.contains("three point") || lower.contains("3pt") || lower.contains("threes") {
        "threes"
    } else if lower.contains("points") || lower.contains("pts") {
        "points"
    } else if lower.contains("rebound") || lower.contains("reb") {
        "rebounds"
    } else if lower.contains("assist") || lower.contains("ast") {
        "assists"
    } else if lower.contains("steal") {
        "steals"
    } else if lower.contains("block") {
        "blocks"
    } else if lower.contains("turnover") {
        "turnovers"
    } else if lower.contains("strikeout") || lower.contains("k's") {
        "strikeouts"
    } else if lower.contains("hits allowed") {
        "hits_allowed"
    } else if lower.contains("total bases") {
        "total_bases"
    } else if lower.contains("rushing yard") {
        "rush_yds"
    } else if lower.contains("passing yard") {
        "pass_yds"
    } else if lower.contains("receiving yard") {
        "rec_yds"
    } else if lower.contains("touchdown") {
        "touchdowns"
    } else if lower.contains("shot") && lower.contains("goal") {
        "sog"
    } else if lower.contains("save") {
        "saves"
    } else if lower.contains("goal") {
        "goals"
    } else {
        return None;
    };

    let name_part = lower
        .split(" total ")
        .next()
        .or_else(|| lower.split(" to ").next())
        .or_else(|| lower.split(" - ").next())
        .unwrap_or(&lower);

    let player = normalize_player(name_part);
    if player.len() < 2 {
        return None;
    }

    Some((player, prop_type.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_nba() {
        assert_eq!(normalize_team("Boston Celtics", "NBA"), "BOS");
        assert_eq!(normalize_team("Celtics", "NBA"), "BOS");
        assert_eq!(normalize_team("Golden State Warriors", "NBA"), "GSW");
        assert_eq!(normalize_team("Warriors", "NBA"), "GSW");
        assert_eq!(normalize_team("Los Angeles Lakers", "NBA"), "LAL");
    }

    #[test]
    fn test_canonical_event_id() {
        let id = canonical_event_id("NBA", "Golden State Warriors", "Boston Celtics", "2026-03-15T19:30:00Z");
        assert_eq!(id, "NBA:2026-03-15T19:BOS-vs-GSW"); // alphabetical: BOS < GSW
    }

    #[test]
    fn test_canonical_same_across_venues() {
        // DK names
        let dk = canonical_event_id("NBA", "Warriors", "Celtics", "2026-03-15T19:30:00Z");
        // FD names
        let fd = canonical_event_id("NBA", "Golden State Warriors", "Boston Celtics", "2026-03-15T19:30:00Z");
        // PIN names (reversed order: away=Warriors, home=Celtics)
        let pin = canonical_event_id("NBA", "Golden State Warriors", "Boston Celtics", "2026-03-15T19:30:00Z");
        assert_eq!(dk, fd);
        assert_eq!(fd, pin);
    }

    #[test]
    fn test_selection_normalization() {
        assert_eq!(normalize_selection("moneyline", "Boston Celtics", "NBA"), "BOS");
        assert_eq!(normalize_selection("total", "Over", "NBA"), "over");
        assert_eq!(normalize_selection("total", "Under 215.5", "NBA"), "under");
    }

    #[test]
    fn test_nhl_suffix_stripped() {
        // "Boston Bruins" maps to BOS-NHL in the dict, but should strip to BOS
        assert_eq!(normalize_team("Boston Bruins", "NHL"), "BOS");
        assert_eq!(normalize_team("Bruins", "NHL"), "BOS");
        // Cross-venue: PIN says "Boston Bruins", DK says "Bruins" → same code
        let pin = canonical_event_id("NHL", "Boston Bruins", "New Jersey Devils", "2026-03-15T19:00:00Z");
        let dk = canonical_event_id("NHL", "Bruins", "Devils", "2026-03-15T19:00:00Z");
        assert_eq!(pin, dk);
    }

    #[test]
    fn test_no_cross_sport_collision() {
        // NBA Boston Celtics and NHL Boston Bruins both strip to BOS,
        // but they're in different sport contexts so event_ids differ
        let nba = canonical_event_id("NBA", "Boston Celtics", "New York Knicks", "2026-03-15T19:00:00Z");
        let nhl = canonical_event_id("NHL", "Boston Bruins", "New York Rangers", "2026-03-15T19:00:00Z");
        assert_ne!(nba, nhl); // Different sport prefix + different opponent codes
    }

    #[test]
    fn test_unknown_team_fallback() {
        // Unknown multi-word names → step 8: first 3 chars of first + last word
        assert_eq!(normalize_team("Some Random Fighter", "UFC"), "SOMFIG");
        // Unknown single word → step 9: up to 4 uppercase chars
        assert_eq!(normalize_team("Topuria", "UFC"), "TOPU");
    }

    #[test]
    fn test_step4_last_word_match() {
        // "Det Pistons" → last word "pistons" matches DET
        assert_eq!(normalize_team("Det Pistons", "NBA"), "DET");
    }

    #[test]
    fn test_step6_strip_brackets() {
        // FD format "(1) Duke" → strip to "duke" → DUKE
        assert_eq!(normalize_team("(1) Duke", "NCAAB"), "DUKE");
    }

    #[test]
    fn test_ncaab_teams() {
        assert_eq!(normalize_team("Duke Blue Devils", "NCAAB"), "DUKE");
        assert_eq!(normalize_team("Gonzaga", "NCAAB"), "GONZ");
        assert_eq!(normalize_team("UConn Huskies", "NCAAB"), "CONN");
        assert_eq!(normalize_team("Kansas Jayhawks", "NCAAB"), "KU");
    }
}
