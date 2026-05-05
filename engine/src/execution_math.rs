#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AuthenticatedQuote {
    pub price: f64,
    pub min_stake: f64,
    pub max_stake: f64,
}

impl AuthenticatedQuote {
    pub fn new(price: f64, min_stake: f64, max_stake: f64) -> Result<Self, String> {
        if !price.is_finite() || price <= 1.0 {
            return Err(format!("invalid authenticated quote price: {:.4}", price));
        }
        if !min_stake.is_finite() || min_stake < 0.0 {
            return Err(format!("invalid min stake: {:.2}", min_stake));
        }
        if !max_stake.is_finite() || max_stake < min_stake {
            return Err(format!(
                "invalid max stake: {:.2} below min {:.2}",
                max_stake, min_stake
            ));
        }
        Ok(Self {
            price,
            min_stake,
            max_stake,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HedgeMath {
    pub pin_stake: f64,
    pub soft_payout: f64,
    pub pin_payout: f64,
    pub total_stake: f64,
    pub guaranteed_profit: f64,
    pub sigma: f64,
}

pub fn round_cents(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

pub fn clamp_stake_to_quote_bounds(raw_stake: f64, min_stake: f64, max_stake: f64) -> f64 {
    round_cents(raw_stake.max(min_stake).min(max_stake))
}

pub fn compute_pin_hedge_from_soft_fill(
    soft_filled_stake: f64,
    soft_filled_odds: f64,
    pin_quote: AuthenticatedQuote,
) -> Option<HedgeMath> {
    if soft_filled_stake <= 0.0 || soft_filled_odds <= 1.0 || pin_quote.price <= 1.0 {
        return None;
    }

    let raw_pin_stake = soft_filled_stake * soft_filled_odds / pin_quote.price;
    let pin_stake =
        clamp_stake_to_quote_bounds(raw_pin_stake, pin_quote.min_stake, pin_quote.max_stake);
    if pin_stake <= 0.0 {
        return None;
    }

    let soft_payout = soft_filled_stake * soft_filled_odds;
    let pin_payout = pin_stake * pin_quote.price;
    let total_stake = soft_filled_stake + pin_stake;
    let guaranteed_profit = soft_payout.min(pin_payout) - total_stake;
    let sigma = 1.0 / soft_filled_odds + 1.0 / pin_quote.price;

    Some(HedgeMath {
        pin_stake,
        soft_payout,
        pin_payout,
        total_stake,
        guaranteed_profit,
        sigma,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hedge_stake_uses_soft_payout_not_profit() {
        let quote = AuthenticatedQuote::new(2.50, 0.0, 999.0).unwrap();
        let hedge = compute_pin_hedge_from_soft_fill(100.0, 1.80, quote).unwrap();

        assert!((hedge.pin_stake - 72.00).abs() < 0.001);
        assert!((hedge.soft_payout - 180.00).abs() < 0.001);
        assert!((hedge.pin_payout - 180.00).abs() < 0.001);
        assert!((hedge.guaranteed_profit - 8.00).abs() < 0.001);
    }

    #[test]
    fn quote_minimum_can_overhedge_without_loss() {
        let quote = AuthenticatedQuote::new(2.50, 80.0, 999.0).unwrap();
        let hedge = compute_pin_hedge_from_soft_fill(100.0, 1.80, quote).unwrap();

        assert_eq!(hedge.pin_stake, 80.0);
        assert_eq!(hedge.pin_payout, 200.0);
        assert_eq!(hedge.soft_payout, 180.0);
        assert_eq!(hedge.guaranteed_profit, 0.0);
    }

    #[test]
    fn quote_maximum_can_make_hedge_unprofitable() {
        let quote = AuthenticatedQuote::new(2.50, 0.0, 50.0).unwrap();
        let hedge = compute_pin_hedge_from_soft_fill(100.0, 1.80, quote).unwrap();

        assert_eq!(hedge.pin_stake, 50.0);
        assert!(hedge.guaranteed_profit < 0.0);
    }

    #[test]
    fn rejects_invalid_quote() {
        assert!(AuthenticatedQuote::new(1.0, 0.0, 999.0).is_err());
        assert!(AuthenticatedQuote::new(2.0, 10.0, 5.0).is_err());
    }
}
