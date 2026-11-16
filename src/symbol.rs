/// Supported spot trading pairs and their display/tick conventions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Symbol {
    BtcUsdt,
    EthUsdt,
    SolUsdt,
    BnbUsdt,
    XrpUsdt,
    AvaxUsdt,
}

impl Symbol {
    pub const ALL: [Symbol; 6] = [
        Symbol::BtcUsdt,
        Symbol::EthUsdt,
        Symbol::SolUsdt,
        Symbol::BnbUsdt,
        Symbol::XrpUsdt,
        Symbol::AvaxUsdt,
    ];

    pub fn stream_id(self) -> &'static str {
        match self {
            Symbol::BtcUsdt => "btcusdt",
            Symbol::EthUsdt => "ethusdt",
            Symbol::SolUsdt => "solusdt",
            Symbol::BnbUsdt => "bnbusdt",
            Symbol::XrpUsdt => "xrpusdt",
            Symbol::AvaxUsdt => "avaxusdt",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Symbol::BtcUsdt => "BTC/USDT",
            Symbol::EthUsdt => "ETH/USDT",
            Symbol::SolUsdt => "SOL/USDT",
            Symbol::BnbUsdt => "BNB/USDT",
            Symbol::XrpUsdt => "XRP/USDT",
            Symbol::AvaxUsdt => "AVAX/USDT",
        }
    }

    pub fn base_asset(self) -> &'static str {
        match self {
            Symbol::BtcUsdt => "BTC",
            Symbol::EthUsdt => "ETH",
            Symbol::SolUsdt => "SOL",
            Symbol::BnbUsdt => "BNB",
            Symbol::XrpUsdt => "XRP",
            Symbol::AvaxUsdt => "AVAX",
        }
    }

    /// Integer scale factor (units per $1.00) used for exact level keying.
    pub fn price_scale(self) -> f64 {
        match self {
            Symbol::XrpUsdt => 10_000.0, // $0.0001 precision
            _ => 100.0,                   // $0.01 precision
        }
    }

    pub fn price_decimals(self) -> usize {
        match self {
            Symbol::XrpUsdt => 4,
            _ => 2,
        }
    }

    pub fn qty_decimals(self) -> usize {
        match self {
            Symbol::BtcUsdt => 4,
            Symbol::EthUsdt | Symbol::BnbUsdt => 3,
            Symbol::SolUsdt | Symbol::AvaxUsdt => 2,
            Symbol::XrpUsdt => 1,
        }
    }

    /// Preset tick-aggregation buckets in scaled integer units.
    pub fn tick_presets(self) -> &'static [(i64, &'static str)] {
        match self {
            Symbol::BtcUsdt => &[
                (1, "$0.01"),
                (10, "$0.10"),
                (100, "$1.00"),
                (500, "$5.00"),
                (1000, "$10.0"),
            ],
            Symbol::EthUsdt => &[
                (1, "$0.01"),
                (10, "$0.10"),
                (50, "$0.50"),
                (100, "$1.00"),
                (500, "$5.00"),
            ],
            Symbol::SolUsdt | Symbol::BnbUsdt => &[
                (1, "$0.01"),
                (5, "$0.05"),
                (10, "$0.10"),
                (50, "$0.50"),
                (100, "$1.00"),
            ],
            Symbol::AvaxUsdt => &[
                (1, "$0.01"),
                (5, "$0.05"),
                (10, "$0.10"),
                (25, "$0.25"),
            ],
            Symbol::XrpUsdt => &[
                (1, "$0.0001"),
                (5, "$0.0005"),
                (10, "$0.0010"),
                (50, "$0.0050"),
                (100, "$0.0100"),
            ],
        }
    }

    pub fn default_sim_qty(self) -> f64 {
        match self {
            Symbol::BtcUsdt => 2.5,
            Symbol::EthUsdt => 25.0,
            Symbol::SolUsdt => 250.0,
            Symbol::BnbUsdt => 50.0,
            Symbol::XrpUsdt => 25_000.0,
            Symbol::AvaxUsdt => 1_000.0,
        }
    }

    pub fn default_whale_usd(self) -> f64 {
        match self {
            Symbol::BtcUsdt => 50_000.0,
            Symbol::EthUsdt => 25_000.0,
            _ => 10_000.0,
        }
    }

    pub fn format_price(self, price: f64) -> String {
        format!("{:.*}", self.price_decimals(), price)
    }

    pub fn format_qty(self, qty: f64) -> String {
        format!("{:.*}", self.qty_decimals(), qty)
    }
}