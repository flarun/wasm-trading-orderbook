//! Executed-trade tape, rolling order-flow statistics, and session CVD.

use std::collections::VecDeque;

/// Rolling window used for flow statistics (60 seconds).
pub const FLOW_WINDOW_MS: u64 = 60_000;
const TAPE_CAPACITY: usize = 2_000;

#[derive(Clone, Debug, PartialEq)]
pub struct Trade {
    pub price: f64,
    pub qty: f64,
    /// Exchange timestamp (ms since epoch).
    pub time_ms: u64,
    /// True when the taker was the buyer (lifted the offer).
    pub is_buy: bool,
}

impl Trade {
    pub fn notional(&self) -> f64 {
        self.price * self.qty
    }

    pub fn format_utc(&self) -> String {
        let total_secs = (self.time_ms / 1000) % 86_400;
        let hours = total_secs / 3600;
        let mins = (total_secs % 3600) / 60;
        let secs = total_secs % 60;
        let millis = self.time_ms % 1000;
        format!("{hours:02}:{mins:02}:{secs:02}.{millis:03}")
    }
}

#[derive(Default, Clone, Debug)]
pub struct FlowStats {
    pub buy_vol: f64,
    pub sell_vol: f64,
    pub buy_count: usize,
    pub sell_count: usize,
    /// Volume-weighted average trade price inside the rolling window.
    pub vwap: f64,
    pub trades_per_sec: f64,
    /// Number of prints at or above the large-trade threshold.
    pub large_trades: usize,
}

impl FlowStats {
    pub fn delta(&self) -> f64 {
        self.buy_vol - self.sell_vol
    }

    /// Share of taker-buy volume, `0.5` when there is no flow.
    pub fn buy_ratio(&self) -> f64 {
        let total = self.buy_vol + self.sell_vol;
        if total > 0.0 {
            self.buy_vol / total
        } else {
            0.5
        }
    }
}

#[derive(Default)]
pub struct TradeTape {
    trades: VecDeque<Trade>,
    /// Session cumulative volume delta (taker buys minus taker sells), base asset.
    pub cvd: f64,
}

impl TradeTape {
    pub fn clear(&mut self) {
        self.trades.clear();
        self.cvd = 0.0;
    }

    pub fn push(&mut self, trade: Trade) {
        self.cvd += if trade.is_buy { trade.qty } else { -trade.qty };
        if self.trades.len() == TAPE_CAPACITY {
            self.trades.pop_front();
        }
        self.trades.push_back(trade);
    }

    /// Most recent trades, newest first.
    pub fn recent(&self, n: usize) -> impl Iterator<Item = &Trade> {
        self.trades.iter().rev().take(n)
    }

    pub fn stats(&self, large_trade_usd: f64) -> FlowStats {
        let Some(newest) = self.trades.back() else {
            return FlowStats::default();
        };
        let cutoff = newest.time_ms.saturating_sub(FLOW_WINDOW_MS);
        let mut s = FlowStats::default();
        let mut price_volume = 0.0;
        let mut oldest_ms = newest.time_ms;

        for t in self.trades.iter().rev() {
            if t.time_ms < cutoff {
                break;
            }
            oldest_ms = t.time_ms;
            price_volume += t.price * t.qty;
            if t.is_buy {
                s.buy_vol += t.qty;
                s.buy_count += 1;
            } else {
                s.sell_vol += t.qty;
                s.sell_count += 1;
            }
            if t.notional() >= large_trade_usd {
                s.large_trades += 1;
            }
        }

        let total_vol = s.buy_vol + s.sell_vol;
        if total_vol > 0.0 {
            s.vwap = price_volume / total_vol;
        }
        let span_secs = ((newest.time_ms.saturating_sub(oldest_ms)) as f64 / 1000.0).max(1.0);
        s.trades_per_sec = (s.buy_count + s.sell_count) as f64 / span_secs;
        s
    }
}