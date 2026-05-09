use crate::config;
use crate::models::*;
use rusqlite::{params, Connection};
use std::sync::Mutex;

pub struct Db {
    conn: Mutex<Connection>,
}

impl Db {
    pub fn open(path: &str) -> Self {
        let conn = Connection::open(path).expect("failed to open sqlite db");
        let db = Db {
            conn: Mutex::new(conn),
        };
        db.migrate();
        db.seed_defaults();
        db
    }

    fn migrate(&self) {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS book_balances (
                venue TEXT PRIMARY KEY,
                balance REAL NOT NULL,
                initial_balance REAL NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS paper_trades (
                id TEXT PRIMARY KEY,
                detected_at TEXT NOT NULL,
                executed_at TEXT NOT NULL,
                event_id TEXT NOT NULL,
                sport TEXT NOT NULL,
                market_type TEXT NOT NULL,
                leg1_venue TEXT NOT NULL,
                leg1_selection TEXT NOT NULL,
                leg1_odds REAL NOT NULL,
                leg1_stake REAL NOT NULL,
                leg2_venue TEXT NOT NULL,
                leg2_selection TEXT NOT NULL,
                leg2_odds REAL NOT NULL,
                leg2_stake REAL NOT NULL,
                sigma REAL NOT NULL,
                theoretical_profit REAL NOT NULL,
                status TEXT NOT NULL DEFAULT 'executed'
            );

            CREATE TABLE IF NOT EXISTS arb_opportunities (
                id TEXT PRIMARY KEY,
                detected_at TEXT NOT NULL,
                event_id TEXT NOT NULL,
                sport TEXT NOT NULL,
                market_type TEXT NOT NULL,
                sigma REAL NOT NULL,
                arb_pct REAL NOT NULL,
                leg1_venue TEXT NOT NULL,
                leg1_selection TEXT NOT NULL,
                leg1_odds REAL NOT NULL,
                leg2_venue TEXT NOT NULL,
                leg2_selection TEXT NOT NULL,
                leg2_odds REAL NOT NULL,
                traded INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
        ",
        )
        .expect("migration failed");
    }

    fn seed_defaults(&self) {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        for venue in config::VENUES {
            conn.execute(
                "INSERT OR IGNORE INTO book_balances (venue, balance, initial_balance, updated_at) VALUES (?1, ?2, ?2, ?3)",
                params![venue, config::DEFAULT_INITIAL_BALANCE, now],
            ).ok();
        }
        let settings_map = vec![
            ("enabled", "true"),
            ("min_arb_pct", "0.5"),
            ("max_stake", "50"),
            ("stake_mode", "fixed"),
            ("fixed_stake", "25"),
            ("proportional_pct", "5"),
        ];
        for (k, v) in settings_map {
            conn.execute(
                "INSERT OR IGNORE INTO settings (key, value) VALUES (?1, ?2)",
                params![k, v],
            )
            .ok();
        }
    }

    // ── Balances ────────────────────────────────────────────────────────

    pub fn get_balances(&self) -> Vec<BookBalance> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT venue, balance, initial_balance, updated_at FROM book_balances ORDER BY venue"
        ).unwrap();
        stmt.query_map([], |row| {
            Ok(BookBalance {
                venue: row.get(0)?,
                balance: row.get(1)?,
                initial_balance: row.get(2)?,
                updated_at: row.get(3)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    pub fn get_balance(&self, venue: &str) -> Option<f64> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT balance FROM book_balances WHERE venue = ?1",
            params![venue],
            |row| row.get(0),
        )
        .ok()
    }

    pub fn update_balance(&self, venue: &str, new_balance: f64) {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE book_balances SET balance = ?1, updated_at = ?2 WHERE venue = ?3",
            params![new_balance, now, venue],
        )
        .ok();
    }

    pub fn reset_balances(&self) {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE book_balances SET balance = initial_balance, updated_at = ?1",
            params![now],
        )
        .ok();
    }

    pub fn set_initial_balance(&self, venue: &str, amount: f64) {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO book_balances (venue, balance, initial_balance, updated_at) VALUES (?1, ?2, ?2, ?3)
             ON CONFLICT(venue) DO UPDATE SET balance = ?2, initial_balance = ?2, updated_at = ?3",
            params![venue, amount, now],
        ).ok();
    }

    // ── Trades ──────────────────────────────────────────────────────────

    pub fn insert_trade(&self, trade: &PaperTrade) {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO paper_trades (id, detected_at, executed_at, event_id, sport, market_type,
             leg1_venue, leg1_selection, leg1_odds, leg1_stake,
             leg2_venue, leg2_selection, leg2_odds, leg2_stake,
             sigma, theoretical_profit, status)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
            params![
                trade.id,
                trade.detected_at,
                trade.executed_at,
                trade.event_id,
                trade.sport,
                trade.market_type,
                trade.leg1_venue,
                trade.leg1_selection,
                trade.leg1_odds,
                trade.leg1_stake,
                trade.leg2_venue,
                trade.leg2_selection,
                trade.leg2_odds,
                trade.leg2_stake,
                trade.sigma,
                trade.theoretical_profit,
                trade.status,
            ],
        )
        .ok();
    }

    pub fn get_trades(&self, limit: u32) -> Vec<PaperTrade> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT id, detected_at, executed_at, event_id, sport, market_type,
             leg1_venue, leg1_selection, leg1_odds, leg1_stake,
             leg2_venue, leg2_selection, leg2_odds, leg2_stake,
             sigma, theoretical_profit, status
             FROM paper_trades ORDER BY executed_at DESC LIMIT ?1",
            )
            .unwrap();
        stmt.query_map(params![limit], |row| {
            Ok(PaperTrade {
                id: row.get(0)?,
                detected_at: row.get(1)?,
                executed_at: row.get(2)?,
                event_id: row.get(3)?,
                sport: row.get(4)?,
                market_type: row.get(5)?,
                leg1_venue: row.get(6)?,
                leg1_selection: row.get(7)?,
                leg1_odds: row.get(8)?,
                leg1_stake: row.get(9)?,
                leg2_venue: row.get(10)?,
                leg2_selection: row.get(11)?,
                leg2_odds: row.get(12)?,
                leg2_stake: row.get(13)?,
                sigma: row.get(14)?,
                theoretical_profit: row.get(15)?,
                status: row.get(16)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    // ── Arb opportunities ───────────────────────────────────────────────

    pub fn insert_arb(&self, arb: &ArbOpportunity) {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO arb_opportunities (id, detected_at, event_id, sport, market_type,
             sigma, arb_pct, leg1_venue, leg1_selection, leg1_odds,
             leg2_venue, leg2_selection, leg2_odds, traded)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
            params![
                arb.id, arb.detected_at, arb.event_id, arb.sport, arb.market_type,
                arb.sigma, arb.arb_pct,
                arb.leg1_venue, arb.leg1_selection, arb.leg1_odds,
                arb.leg2_venue, arb.leg2_selection, arb.leg2_odds,
                arb.traded as i32,
            ],
        ).ok();
    }

    pub fn mark_arb_traded(&self, id: &str) {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE arb_opportunities SET traded = 1 WHERE id = ?1",
            params![id],
        )
        .ok();
    }

    pub fn get_arbs(&self, limit: u32) -> Vec<ArbOpportunity> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT id, detected_at, event_id, sport, market_type, sigma, arb_pct,
             leg1_venue, leg1_selection, leg1_odds, leg2_venue, leg2_selection, leg2_odds, traded
             FROM arb_opportunities ORDER BY detected_at DESC LIMIT ?1",
            )
            .unwrap();
        stmt.query_map(params![limit], |row| {
            Ok(ArbOpportunity {
                id: row.get(0)?,
                detected_at: row.get(1)?,
                event_id: row.get(2)?,
                sport: row.get(3)?,
                market_type: row.get(4)?,
                sigma: row.get(5)?,
                arb_pct: row.get(6)?,
                leg1_venue: row.get(7)?,
                leg1_selection: row.get(8)?,
                leg1_odds: row.get(9)?,
                leg2_venue: row.get(10)?,
                leg2_selection: row.get(11)?,
                leg2_odds: row.get(12)?,
                traded: row.get::<_, i32>(13)? != 0,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    // ── Settings ────────────────────────────────────────────────────────

    pub fn get_settings(&self) -> Settings {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT key, value FROM settings").unwrap();
        let rows: std::collections::HashMap<String, String> = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        Settings {
            enabled: rows.get("enabled").map(|v| v == "true").unwrap_or(true),
            min_arb_pct: rows
                .get("min_arb_pct")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.5),
            max_stake: rows
                .get("max_stake")
                .and_then(|v| v.parse().ok())
                .unwrap_or(50.0),
            stake_mode: rows
                .get("stake_mode")
                .cloned()
                .unwrap_or_else(|| "fixed".into()),
            fixed_stake: rows
                .get("fixed_stake")
                .and_then(|v| v.parse().ok())
                .unwrap_or(25.0),
            proportional_pct: rows
                .get("proportional_pct")
                .and_then(|v| v.parse().ok())
                .unwrap_or(5.0),
        }
    }

    pub fn update_settings(&self, settings: &Settings) {
        let conn = self.conn.lock().unwrap();
        let pairs: Vec<(&str, String)> = vec![
            ("enabled", settings.enabled.to_string()),
            ("min_arb_pct", settings.min_arb_pct.to_string()),
            ("max_stake", settings.max_stake.to_string()),
            ("stake_mode", settings.stake_mode.clone()),
            ("fixed_stake", settings.fixed_stake.to_string()),
            ("proportional_pct", settings.proportional_pct.to_string()),
        ];
        for (k, v) in pairs {
            conn.execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = ?2",
                params![k, v],
            ).ok();
        }
    }

    // ── Stats ───────────────────────────────────────────────────────────

    pub fn get_stats(&self) -> Stats {
        let conn = self.conn.lock().unwrap();
        let total_pnl: f64 = conn
            .query_row(
                "SELECT COALESCE(SUM(theoretical_profit), 0) FROM paper_trades",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0.0);
        let trade_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM paper_trades", [], |row| row.get(0))
            .unwrap_or(0);
        let avg_arb_pct: f64 = conn
            .query_row(
                "SELECT COALESCE(AVG((1.0 - sigma) * 100), 0) FROM paper_trades",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0.0);
        let best_trade_profit: f64 = conn
            .query_row(
                "SELECT COALESCE(MAX(theoretical_profit), 0) FROM paper_trades",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0.0);
        let total_arbs_detected: i64 = conn
            .query_row("SELECT COUNT(*) FROM arb_opportunities", [], |row| {
                row.get(0)
            })
            .unwrap_or(0);
        let total_deployed: f64 = conn
            .query_row(
                "SELECT COALESCE(SUM(leg1_stake + leg2_stake), 0) FROM paper_trades",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0.0);

        Stats {
            total_pnl,
            trade_count,
            avg_arb_pct,
            best_trade_profit,
            total_arbs_detected,
            total_deployed,
        }
    }

    // ── Reset ───────────────────────────────────────────────────────────

    pub fn full_reset(&self) {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "
            DELETE FROM paper_trades;
            DELETE FROM arb_opportunities;
        ",
        )
        .ok();
        drop(conn);
        self.reset_balances();
    }
}
