#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BookStatus {
    Open,
    LivePending,
    CashedOut,
    Voided,
    Settled,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CashoutConfirmation {
    ConfirmedCashout,
    TerminalNonCashout,
    StillOpen,
    PurchaseNotFound,
    Unknown,
}

impl BookStatus {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            BookStatus::CashedOut | BookStatus::Voided | BookStatus::Settled
        )
    }
}

pub fn classify_draftkings_raw_status(raw_status: &str) -> BookStatus {
    let raw = raw_status.trim().to_ascii_lowercase();
    if raw.is_empty() {
        return BookStatus::Unknown;
    }
    let compact = raw
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>();

    let live_pending =
        has_word(&raw, "pending") || has_word(&raw, "processing") || compact.contains("livedelay");

    if has_word(&raw, "unsettled")
        || has_phrase(&raw, &["not", "settled"])
        || has_word(&raw, "open")
        || has_word(&raw, "placed")
        || has_word(&raw, "accepted")
        || has_word(&raw, "active")
        || live_pending
    {
        return if live_pending {
            BookStatus::LivePending
        } else {
            BookStatus::Open
        };
    }

    if compact.contains("cashedout") || has_phrase(&raw, &["cash", "out"]) {
        return BookStatus::CashedOut;
    }

    if has_word(&raw, "void")
        || has_word(&raw, "voided")
        || has_word(&raw, "cancel")
        || has_word(&raw, "canceled")
        || has_word(&raw, "cancelled")
        || has_word(&raw, "refund")
        || has_word(&raw, "refunded")
        || has_word(&raw, "push")
    {
        return BookStatus::Voided;
    }

    if has_word(&raw, "settled")
        || has_word(&raw, "closed")
        || has_word(&raw, "complete")
        || has_word(&raw, "completed")
        || has_word(&raw, "resulted")
        || has_word(&raw, "paid")
        || has_word(&raw, "graded")
        || has_word(&raw, "won")
        || has_word(&raw, "win")
        || has_word(&raw, "winner")
        || has_word(&raw, "lost")
        || has_word(&raw, "loss")
        || has_word(&raw, "loser")
    {
        return BookStatus::Settled;
    }

    BookStatus::Unknown
}

pub fn classify_cashout_confirmation(
    raw_status: Option<&str>,
    _normalized_status: Option<&str>,
    error: Option<&str>,
) -> CashoutConfirmation {
    if let Some(raw) = raw_status {
        match classify_draftkings_raw_status(raw) {
            BookStatus::CashedOut => return CashoutConfirmation::ConfirmedCashout,
            BookStatus::Voided | BookStatus::Settled => {
                return CashoutConfirmation::TerminalNonCashout
            }
            BookStatus::Open | BookStatus::LivePending => return CashoutConfirmation::StillOpen,
            BookStatus::Unknown => {}
        }
    }

    let err = error.unwrap_or_default().to_ascii_lowercase();
    if err.contains("purchase_not_found") {
        CashoutConfirmation::PurchaseNotFound
    } else {
        CashoutConfirmation::Unknown
    }
}

fn has_phrase(raw: &str, phrase: &[&str]) -> bool {
    let words = words(raw);
    if phrase.is_empty() || words.len() < phrase.len() {
        return false;
    }
    words
        .windows(phrase.len())
        .any(|window| window.iter().copied().eq(phrase.iter().copied()))
}

fn has_word(raw: &str, needle: &str) -> bool {
    words(raw).into_iter().any(|word| word == needle)
}

fn words(raw: &str) -> Vec<&str> {
    raw.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|part| !part.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_open_statuses_before_settled_substrings() {
        let cases = [
            ("Unsettled", BookStatus::Open),
            ("Not Settled", BookStatus::Open),
            ("Open", BookStatus::Open),
            ("Placed", BookStatus::Open),
            ("Accepted", BookStatus::Open),
            ("Pending", BookStatus::LivePending),
            ("Processing", BookStatus::LivePending),
            ("LiveDelay", BookStatus::LivePending),
            ("Live Delay", BookStatus::LivePending),
            ("Unsettled - LiveDelay", BookStatus::LivePending),
        ];

        for (raw, expected) in cases {
            assert_eq!(classify_draftkings_raw_status(raw), expected, "{raw}");
            assert!(!classify_draftkings_raw_status(raw).is_terminal(), "{raw}");
        }
    }

    #[test]
    fn uses_word_boundaries_for_open_statuses() {
        assert_eq!(
            classify_draftkings_raw_status("preunsettledfoo"),
            BookStatus::Unknown
        );
        assert_eq!(
            classify_draftkings_raw_status("settlementPending"),
            BookStatus::Unknown
        );
    }

    #[test]
    fn separates_cashout_from_game_settlement() {
        assert_eq!(
            classify_draftkings_raw_status("Cashed Out"),
            BookStatus::CashedOut
        );
        assert_eq!(
            classify_draftkings_raw_status("CashedOut"),
            BookStatus::CashedOut
        );
        assert_eq!(
            classify_draftkings_raw_status("Settled"),
            BookStatus::Settled
        );
        assert_eq!(
            classify_draftkings_raw_status("Settled - Lost"),
            BookStatus::Settled
        );
        assert_eq!(
            classify_draftkings_raw_status("Closed"),
            BookStatus::Settled
        );
        assert_eq!(classify_draftkings_raw_status("Void"), BookStatus::Voided);
        assert_eq!(
            classify_draftkings_raw_status("Refunded"),
            BookStatus::Voided
        );
    }

    #[test]
    fn cashout_confirmation_trusts_raw_status_first() {
        assert_eq!(
            classify_cashout_confirmation(Some("Cashed Out"), Some("settled"), None),
            CashoutConfirmation::ConfirmedCashout
        );
        assert_eq!(
            classify_cashout_confirmation(Some("Unsettled"), Some("settled"), None),
            CashoutConfirmation::StillOpen
        );
        assert_eq!(
            classify_cashout_confirmation(Some("Settled"), Some("settled"), None),
            CashoutConfirmation::TerminalNonCashout
        );
        assert_eq!(
            classify_cashout_confirmation(None, Some("unknown"), Some("purchase_not_found")),
            CashoutConfirmation::PurchaseNotFound
        );
    }
}
