use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::models::ArbCandidate;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ArbState {
    CandidateDetected,
    Sizing,
    CapitalReserved,
    Leg1Submitted,
    Leg1Awaiting,
    Leg1Filled { accepted_stake: f64, accepted_odds: f64 },
    Leg1Partial { accepted_stake: f64, accepted_odds: f64 },
    Leg1Rejected,
    Leg1Unknown,
    Leg2Submitted,
    Leg2Awaiting,
    ExecutionClosed,
    RescueNeeded,
    RescueComputed,
    RescueSubmitted,
    RescueClosed,
    AbortAndEscalate,
    AwaitingSettlement,
    Reconciled,
}

impl std::fmt::Display for ArbState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ArbState::CandidateDetected => write!(f, "CANDIDATE_DETECTED"),
            ArbState::Sizing => write!(f, "SIZING"),
            ArbState::CapitalReserved => write!(f, "CAPITAL_RESERVED"),
            ArbState::Leg1Submitted => write!(f, "LEG1_SUBMITTED"),
            ArbState::Leg1Awaiting => write!(f, "LEG1_AWAITING"),
            ArbState::Leg1Filled { .. } => write!(f, "LEG1_FILLED"),
            ArbState::Leg1Partial { .. } => write!(f, "LEG1_PARTIAL"),
            ArbState::Leg1Rejected => write!(f, "LEG1_REJECTED"),
            ArbState::Leg1Unknown => write!(f, "LEG1_UNKNOWN"),
            ArbState::Leg2Submitted => write!(f, "LEG2_SUBMITTED"),
            ArbState::Leg2Awaiting => write!(f, "LEG2_AWAITING"),
            ArbState::ExecutionClosed => write!(f, "EXECUTION_CLOSED"),
            ArbState::RescueNeeded => write!(f, "RESCUE_NEEDED"),
            ArbState::RescueComputed => write!(f, "RESCUE_COMPUTED"),
            ArbState::RescueSubmitted => write!(f, "RESCUE_SUBMITTED"),
            ArbState::RescueClosed => write!(f, "RESCUE_CLOSED"),
            ArbState::AbortAndEscalate => write!(f, "ABORT_AND_ESCALATE"),
            ArbState::AwaitingSettlement => write!(f, "AWAITING_SETTLEMENT"),
            ArbState::Reconciled => write!(f, "RECONCILED"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateTransition {
    pub from: String,
    pub to: String,
    pub timestamp: DateTime<Utc>,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArbAttempt {
    pub id: Uuid,
    pub state: ArbState,
    pub detected_at: DateTime<Utc>,

    pub event_id: String,
    pub sport: String,
    pub market_type: String,

    // Leg 1
    pub leg1_venue: String,
    pub leg1_selection: String,
    pub leg1_odds: f64,
    pub leg1_stake: f64,
    pub leg1_status: Option<String>,
    pub leg1_accepted_stake: Option<f64>,
    pub leg1_accepted_odds: Option<f64>,

    // Leg 2
    pub leg2_venue: String,
    pub leg2_selection: String,
    pub leg2_odds: f64,
    pub leg2_stake: f64,
    pub leg2_status: Option<String>,
    pub leg2_accepted_stake: Option<f64>,
    pub leg2_accepted_odds: Option<f64>,

    // Math
    pub sigma: f64,
    pub arb_pct: f64,
    pub theoretical_profit: f64,
    pub realized_pnl: Option<f64>,

    // Rescue
    pub rescue_needed: bool,
    pub rescue_venue: Option<String>,
    pub rescue_stake: Option<f64>,
    pub rescue_odds: Option<f64>,
    pub rescue_status: Option<String>,

    pub is_live: bool,
    pub event_exposure: Option<f64>,
    pub execution_mode: String,
    pub state_timeline: Vec<StateTransition>,
}

impl ArbAttempt {
    pub fn new(candidate: &ArbCandidate, leg1_stake: f64, leg2_stake: f64, profit: f64) -> Self {
        let now = Utc::now();
        ArbAttempt {
            id: Uuid::new_v4(),
            state: ArbState::CandidateDetected,
            detected_at: now,
            event_id: candidate.event_id.clone(),
            sport: candidate.sport.clone(),
            market_type: candidate.market_type.clone(),
            leg1_venue: candidate.leg1_venue.clone(),
            leg1_selection: candidate.leg1_selection.clone(),
            leg1_odds: candidate.leg1_odds,
            leg1_stake,
            leg1_status: None,
            leg1_accepted_stake: None,
            leg1_accepted_odds: None,
            leg2_venue: candidate.leg2_venue.clone(),
            leg2_selection: candidate.leg2_selection.clone(),
            leg2_odds: candidate.leg2_odds,
            leg2_stake,
            leg2_status: None,
            leg2_accepted_stake: None,
            leg2_accepted_odds: None,
            sigma: candidate.sigma,
            arb_pct: candidate.arb_pct,
            theoretical_profit: profit,
            realized_pnl: None,
            rescue_needed: false,
            rescue_venue: None,
            rescue_stake: None,
            rescue_odds: None,
            rescue_status: None,
            is_live: candidate.is_live,
            event_exposure: None,
            execution_mode: "paper".to_string(),
            state_timeline: vec![StateTransition {
                from: "IDLE".into(),
                to: "CANDIDATE_DETECTED".into(),
                timestamp: now,
                note: format!("sigma={:.4}, arb={:.2}%", candidate.sigma, candidate.arb_pct * 100.0),
            }],
        }
    }

    pub fn transition(&mut self, new_state: ArbState, note: &str) {
        let from = format!("{}", self.state);
        let to = format!("{}", new_state);
        tracing::info!(
            attempt_id = %self.id,
            from = %from,
            to = %to,
            "State: {} → {}: {}",
            from, to, note
        );
        self.state_timeline.push(StateTransition {
            from,
            to,
            timestamp: Utc::now(),
            note: note.to_string(),
        });
        self.state = new_state;
    }
}
