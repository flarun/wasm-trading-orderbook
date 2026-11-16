//! Limit order book state, price aggregation, microstructure metrics, and
//! pre-trade market-impact simulation.

use std::collections::BTreeMap;

/// Number of top-of-book levels used for the Order Book Imbalance (OBI) metric.
pub const OBI_LEVELS: usize = 10;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Level {
    pub price: f64,
    pub qty: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Buy,
    Sell,
}

/// Price-bucketed view of the book, both sides sorted best-first.
#[derive(Default, Clone, Debug)]
pub struct Ladder {
    pub bids: Vec<Level>,
    pub asks: Vec<Level>,
}

/// Tick-size independent top-of-book analytics (computed on raw levels).
#[derive(Clone, Copy, Debug, Default)]
pub struct Metrics {
    pub best_bid: Option<Level>,
    pub best_ask: Option<Level>,
    pub mid: f64,
    pub spread: f64,
    pub spread_bps: f64,
    /// Size-weighted fair value: leans toward the thinner side of the touch.
    pub microprice: f64,
    pub micro_delta_bps: f64,
    /// Bid share of the top `OBI_LEVELS` levels, in `0.0..=1.0`.
    pub obi: f64,
    /// Total resting notional (USDT) across visible levels.
    pub bid_notional: f64,
    pub ask_notional: f64,
}

impl Metrics {
    /// OBI mapped to `-100..=+100` (positive = bid heavy).
    pub fn obi_signed_pct(&self) -> f64 {
        (self.obi - 0.5) * 200.0
    }
}

/// Result of sweeping the book with a simulated market order.
#[derive(Clone, Copy, Debug)]
pub struct Fill {
    pub side: Side,
    pub requested_qty: f64,
    pub filled_qty: f64,
    pub vwap: f64,
    pub best_price: f64,
    pub worst_price: f64,
    pub notional: f64,
    pub levels_used: usize,
    pub fully_filled: bool,
}

impl Fill {
    /// Execution cost versus the mid price in bps (positive = worse than mid).
    pub fn slippage_vs_mid_bps(&self, mid: f64) -> f64 {
        if self.filled_qty <= 0.0 || mid <= 0.0 {
            return 0.0;
        }
        let diff = match self.side {
            Side::Buy => self.vwap - mid,
            Side::Sell => mid - self.vwap,
        };
        diff / mid * 10_000.0
    }

    /// Price impact versus the best touch price in bps (positive = worse).
    pub fn impact_vs_touch_bps(&self) -> f64 {
        if self.filled_qty <= 0.0 || self.best_price <= 0.0 {
            return 0.0;
        }
        let diff = match self.side {
            Side::Buy => self.vwap - self.best_price,
            Side::Sell => self.best_price - self.vwap,
        };
        diff / self.best_price * 10_000.0
    }

    pub fn fee(&self, fee_bps: f64) -> f64 {
        self.notional * fee_bps / 10_000.0
    }

    /// Buy: USDT paid including fee. Sell: USDT received after fee.
    pub fn net_usdt(&self, fee_bps: f64) -> f64 {
        let f = self.fee(fee_bps);
        match self.side {
            Side::Buy => self.notional + f,
            Side::Sell => (self.notional - f).max(0.0),
        }
    }
}

#[derive(Default)]
pub struct OrderBook {
    scale: f64,
    bids: BTreeMap<i64, f64>,
    asks: BTreeMap<i64, f64>,
}

impl OrderBook {
    pub fn new(scale: f64) -> Self {
        Self {
            scale: scale.max(1.0),
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
        }
    }

    pub fn clear(&mut self) {
        self.bids.clear();
        self.asks.clear();
    }

    pub fn set_scale(&mut self, scale: f64) {
        self.scale = scale.max(1.0);
        self.clear();
    }

    pub fn apply_snapshot(&mut self, bids: &[[String; 2]], asks: &[[String; 2]]) {
        self.bids.clear();
        self.asks.clear();
        Self::insert_levels(&mut self.bids, bids, self.scale);
        Self::insert_levels(&mut self.asks, asks, self.scale);
    }

    fn insert_levels(side: &mut BTreeMap<i64, f64>, levels: &[[String; 2]], scale: f64) {
        for [p_str, q_str] in levels {
            if let (Ok(p), Ok(q)) = (p_str.parse::<f64>(), q_str.parse::<f64>()) {
                if q > 0.0 && p > 0.0 {
                    let key = (p * scale).round() as i64;
                    side.insert(key, q);
                }
            }
        }
    }

    /// Computes tick-independent top-of-book analytics from raw levels.
    pub fn metrics(&self) -> Metrics {
        let best_bid = self.bids.iter().next_back().map(|(&k, &qty)| Level {
            price: k as f64 / self.scale,
            qty,
        });
        let best_ask = self.asks.iter().next().map(|(&k, &qty)| Level {
            price: k as f64 / self.scale,
            qty,
        });

        let (Some(bb), Some(ba)) = (best_bid, best_ask) else {
            return Metrics::default();
        };

        let mid = (bb.price + ba.price) * 0.5;
        let spread = (ba.price - bb.price).max(0.0);
        let spread_bps = if mid > 0.0 {
            spread / mid * 10_000.0
        } else {
            0.0
        };

        let touch_vol = bb.qty + ba.qty;
        let microprice = if touch_vol > 0.0 {
            (bb.qty * ba.price + ba.qty * bb.price) / touch_vol
        } else {
            mid
        };
        let micro_delta_bps = if mid > 0.0 {
            (microprice - mid) / mid * 10_000.0
        } else {
            0.0
        };

        let bid_vol_top: f64 = self.bids.values().rev().take(OBI_LEVELS).sum();
        let ask_vol_top: f64 = self.asks.values().take(OBI_LEVELS).sum();
        let obi = if bid_vol_top + ask_vol_top > 0.0 {
            bid_vol_top / (bid_vol_top + ask_vol_top)
        } else {
            0.5
        };

        let bid_notional = self
            .bids
            .iter()
            .map(|(&k, &q)| (k as f64 / self.scale) * q)
            .sum();
        let ask_notional = self
            .asks
            .iter()
            .map(|(&k, &q)| (k as f64 / self.scale) * q)
            .sum();

        Metrics {
            best_bid: Some(bb),
            best_ask: Some(ba),
            mid,
            spread,
            spread_bps,
            microprice,
            micro_delta_bps,
            obi,
            bid_notional,
            ask_notional,
        }
    }

    /// Aggregates raw price levels into bucketed ticks (`bucket_units >= 1`).
    pub fn ladder(&self, bucket_units: i64) -> Ladder {
        let b = bucket_units.max(1);

        let mut agg_bids: BTreeMap<i64, f64> = BTreeMap::new();
        for (&p, &q) in &self.bids {
            let bucket = p.div_euclid(b) * b;
            *agg_bids.entry(bucket).or_insert(0.0) += q;
        }

        let mut agg_asks: BTreeMap<i64, f64> = BTreeMap::new();
        for (&p, &q) in &self.asks {
            let bucket = (p + b - 1).div_euclid(b) * b;
            *agg_asks.entry(bucket).or_insert(0.0) += q;
        }

        Ladder {
            bids: agg_bids
                .into_iter()
                .rev()
                .map(|(p, q)| Level {
                    price: p as f64 / self.scale,
                    qty: q,
                })
                .collect(),
            asks: agg_asks
                .into_iter()
                .map(|(p, q)| Level {
                    price: p as f64 / self.scale,
                    qty: q,
                })
                .collect(),
        }
    }

    /// Simulates sweeping the raw book with a market order of `qty` base units.
    pub fn simulate_market_order(&self, side: Side, qty: f64) -> Fill {
        let levels_iter: Box<dyn Iterator<Item = Level>> = match side {
            Side::Buy => Box::new(self.asks.iter().map(|(&k, &q)| Level {
                price: k as f64 / self.scale,
                qty: q,
            })),
            Side::Sell => Box::new(self.bids.iter().rev().map(|(&k, &q)| Level {
                price: k as f64 / self.scale,
                qty: q,
            })),
        };

        let mut remaining = qty.max(0.0);
        let mut filled = 0.0;
        let mut notional = 0.0;
        let mut best_price = 0.0;
        let mut worst_price = 0.0;
        let mut levels_used = 0;

        for lvl in levels_iter {
            if remaining <= 1e-12 {
                break;
            }
            if levels_used == 0 {
                best_price = lvl.price;
            }
            let take = remaining.min(lvl.qty);
            notional += take * lvl.price;
            filled += take;
            remaining -= take;
            worst_price = lvl.price;
            levels_used += 1;
        }

        let vwap = if filled > 0.0 { notional / filled } else { 0.0 };

        Fill {
            side,
            requested_qty: qty,
            filled_qty: filled,
            vwap,
            best_price,
            worst_price,
            notional,
            levels_used,
            fully_filled: remaining <= 1e-8 && filled > 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_microprice_and_sweep() {
        let mut book = OrderBook::new(100.0);
        book.apply_snapshot(
            &[["100.00".into(), "10.0".into()], ["99.90".into(), "5.0".into()]],
            &[["100.10".into(), "2.0".into()], ["100.20".into(), "8.0".into()]],
        );

        let m = book.metrics();
        assert!((m.mid - 100.05).abs() < 1e-6);
        // Heavy bid (10) vs thin ask (2) pushes microprice closer to ask
        assert!(m.microprice > m.mid);

        let fill = book.simulate_market_order(Side::Buy, 5.0);
        assert!(fill.fully_filled);
        assert_eq!(fill.levels_used, 2);
        // 2 @ 100.10 + 3 @ 100.20 = 200.20 + 300.60 = 500.80 -> VWAP 100.16
        assert!((fill.vwap - 100.16).abs() < 1e-6);
    }
}