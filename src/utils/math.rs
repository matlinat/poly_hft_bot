/// Expected locked-in profit for a completed two-leg position, in quote currency.
///
/// `leg1_price` and `leg2_price` are per-share prices (0-1 for binary markets),
/// `shares` is the number of shares on each side, `fee_rate` is proportional fee (e.g. 0.02).
pub fn locked_profit(
    leg1_price: f64,
    leg2_price: f64,
    shares: f64,
    fee_rate: f64,
) -> f64 {
    let cost = (leg1_price + leg2_price) * shares;
    let payoff = shares; // 1.0 per winning share
    let gross = payoff - cost;
    let fees = cost * fee_rate;
    gross - fees
}

/// Simple Kelly fraction for a binary bet.
///
/// `p` is win probability, `b` is net odds (e.g. b = (1/price) - 1).
/// Returns a fraction of capital to risk; negative means no bet.
pub fn kelly_fraction(p: f64, b: f64) -> f64 {
    if b <= 0.0 {
        return 0.0;
    }
    let f = (p * (b + 1.0) - 1.0) / b;
    if f.is_sign_negative() { 0.0 } else { f }
}

/// Cap Kelly sizing by risk-per-trade and available capital.
pub fn position_size_kelly(
    capital: f64,
    price: f64,
    p: f64,
    fee_rate: f64,
    risk_per_trade_pct: f64,
) -> f64 {
    if capital <= 0.0 || price <= 0.0 {
        return 0.0;
    }
    // Effective odds given fees is approximate; use b ~ (1 - price) / price.
    let b = (1.0 - price) / price;
    let k = kelly_fraction(p, b);
    if k <= 0.0 {
        return 0.0;
    }
    let max_risk = capital * (risk_per_trade_pct / 100.0);
    let stake = (capital * k).min(max_risk);
    let cost_per_share = price * (1.0 + fee_rate);
    stake / cost_per_share
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_locked_profit_positive() {
        let p = locked_profit(0.4, 0.5, 20.0, 0.02);
        assert!(p > 0.0);
    }

    #[test]
    fn test_kelly_basic() {
        let f = kelly_fraction(0.55, 1.0);
        assert!(f > 0.0);
    }

    #[test]
    fn test_position_size_caps() {
        let shares = position_size_kelly(10_000.0, 0.5, 0.55, 0.02, 2.0);
        assert!(shares > 0.0);
        let shares_small = position_size_kelly(100.0, 0.9, 0.55, 0.02, 2.0);
        assert!(shares_small > 0.0);
    }
}

