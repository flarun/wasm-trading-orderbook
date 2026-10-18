use eframe::egui;
use ewebsock::{WsEvent, WsMessage, WsReceiver, WsSender};
use serde::Deserialize;
use std::collections::{BTreeMap, VecDeque};

fn main() -> eframe::Result<()> {
    #[cfg(target_arch = "wasm32")]
    {
        eframe::WebLogger::init(log::LevelFilter::Debug).ok();
        let web_options = eframe::WebOptions::default();
        wasm_bindgen_futures::spawn_local(async {
            eframe::WebRunner::new()
                .start(
                    "the_canvas_id",
                    web_options,
                    Box::new(|cc| Box::new(OrderbookApp::new(cc))),
                )
                .await
                .expect("Failed to start eframe");
        });
        return Ok(());
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        let options = eframe::NativeOptions::default();
        eframe::run_native(
            "Wasm Trading Orderbook",
            options,
            Box::new(|cc| Box::new(OrderbookApp::new(cc))),
        )
    }
}

#[derive(Deserialize)]
struct DepthUpdate {
    bids: Vec<[String; 2]>,
    asks: Vec<[String; 2]>,
}

#[derive(Deserialize)]
struct AggTradeEvent {
    #[serde(rename = "p")]
    price: String,
    #[serde(rename = "q")]
    qty: String,
    #[serde(rename = "T")]
    timestamp_ms: u64,
    #[serde(rename = "m")]
    is_buyer_maker: bool, // true = Seller was Taker (Red), false = Buyer was Taker (Green)
}

#[derive(Clone)]
struct TradePrint {
    price: f64,
    qty: f64,
    time_str: String,
    is_buy_aggressor: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum Symbol {
    BtcUsdt,
    EthUsdt,
    SolUsdt,
    BnbUsdt,
}

impl Symbol {
    fn as_stream_id(&self) -> &'static str {
        match self {
            Symbol::BtcUsdt => "btcusdt",
            Symbol::EthUsdt => "ethusdt",
            Symbol::SolUsdt => "solusdt",
            Symbol::BnbUsdt => "bnbusdt",
        }
    }

    fn label(&self) -> &'static str {
        match self {
            Symbol::BtcUsdt => "BTC/USDT",
            Symbol::EthUsdt => "ETH/USDT",
            Symbol::SolUsdt => "SOL/USDT",
            Symbol::BnbUsdt => "BNB/USDT",
        }
    }

    fn base_asset(&self) -> &'static str {
        match self {
            Symbol::BtcUsdt => "BTC",
            Symbol::EthUsdt => "ETH",
            Symbol::SolUsdt => "SOL",
            Symbol::BnbUsdt => "BNB",
        }
    }

    fn default_sim_qty(&self) -> f64 {
        match self {
            Symbol::BtcUsdt => 2.5,
            Symbol::EthUsdt => 25.0,
            Symbol::SolUsdt => 250.0,
            Symbol::BnbUsdt => 50.0,
        }
    }
}

struct WalkResult {
    vwap: f64,
    worst_price: f64,
    filled_qty: f64,
    slippage_bps: f64,
    fully_filled: bool,
}

struct OrderbookApp {
    symbol: Symbol,
    _depth_sender: WsSender,
    depth_receiver: WsReceiver,
    _trade_sender: WsSender,
    trade_receiver: WsReceiver,

    // Raw prices stored in integer cents (Price * 100)
    raw_bids: BTreeMap<i64, f64>,
    raw_asks: BTreeMap<i64, f64>,
    trades: VecDeque<TradePrint>,

    // User Controls
    tick_bucket_cents: i64, // 1 = $0.01, 10 = $0.10, 100 = $1.00, 500 = $5.00
    paused: bool,
    sim_order_qty: f64,

    // Telemetry
    depth_msg_count: u64,
    trade_msg_count: u64,
    msgs_this_sec: u64,
    msgs_per_sec: u64,
    last_sec_timestamp: f64,
}

impl OrderbookApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = egui::Color32::from_rgb(13, 17, 23);
        visuals.window_fill = egui::Color32::from_rgb(13, 17, 23);
        cc.egui_ctx.set_visuals(visuals);

        let symbol = Symbol::BtcUsdt;
        let (depth_sender, depth_receiver, trade_sender, trade_receiver) =
            Self::connect_streams(symbol);

        Self {
            symbol,
            _depth_sender: depth_sender,
            depth_receiver,
            _trade_sender: trade_sender,
            trade_receiver,
            raw_bids: BTreeMap::new(),
            raw_asks: BTreeMap::new(),
            trades: VecDeque::with_capacity(60),
            tick_bucket_cents: 1,
            paused: false,
            sim_order_qty: symbol.default_sim_qty(),
            depth_msg_count: 0,
            trade_msg_count: 0,
            msgs_this_sec: 0,
            msgs_per_sec: 0,
            last_sec_timestamp: 0.0,
        }
    }

    fn connect_streams(symbol: Symbol) -> (WsSender, WsReceiver, WsSender, WsReceiver) {
        let stream_id = symbol.as_stream_id();
        let depth_url = format!("wss://stream.binance.com:9443/ws/{}@depth20@100ms", stream_id);
        let trade_url = format!("wss://stream.binance.com:9443/ws/{}@aggTrade", stream_id);

        let (depth_sender, depth_receiver) =
            ewebsock::connect(depth_url, ewebsock::Options::default())
                .expect("Failed to connect to Depth WebSocket");
        let (trade_sender, trade_receiver) =
            ewebsock::connect(trade_url, ewebsock::Options::default())
                .expect("Failed to connect to Trade WebSocket");

        (depth_sender, depth_receiver, trade_sender, trade_receiver)
    }

    fn switch_symbol(&mut self, new_symbol: Symbol) {
        if self.symbol == new_symbol {
            return;
        }
        self.symbol = new_symbol;
        self.raw_bids.clear();
        self.raw_asks.clear();
        self.trades.clear();
        self.tick_bucket_cents = 1;
        self.sim_order_qty = new_symbol.default_sim_qty();

        let (ds, dr, ts, tr) = Self::connect_streams(new_symbol);
        self._depth_sender = ds;
        self.depth_receiver = dr;
        self._trade_sender = ts;
        self.trade_receiver = tr;
    }

    fn update_levels(book: &mut BTreeMap<i64, f64>, raw_levels: Vec<[String; 2]>) {
        for entry in raw_levels {
            if let (Ok(p), Ok(q)) = (entry[0].parse::<f64>(), entry[1].parse::<f64>()) {
                let price_key = (p * 100.0).round() as i64;
                if q > 0.0 {
                    book.insert(price_key, q);
                }
            }
        }
    }

    fn aggregated_bids(&self) -> Vec<(f64, f64)> {
        let b = self.tick_bucket_cents.max(1);
        let mut agg: BTreeMap<i64, f64> = BTreeMap::new();
        for (&p, &q) in &self.raw_bids {
            // Floor bids to bucket boundary
            let bucketed = (p / b) * b;
            *agg.entry(bucketed).or_insert(0.0) += q;
        }
        agg.into_iter()
            .rev()
            .map(|(p, q)| (p as f64 / 100.0, q))
            .collect()
    }

    fn aggregated_asks(&self) -> Vec<(f64, f64)> {
        let b = self.tick_bucket_cents.max(1);
        let mut agg: BTreeMap<i64, f64> = BTreeMap::new();
        for (&p, &q) in &self.raw_asks {
            // Ceil asks to bucket boundary
            let bucketed = ((p + b - 1) / b) * b;
            *agg.entry(bucketed).or_insert(0.0) += q;
        }
        agg.into_iter()
            .map(|(p, q)| (p as f64 / 100.0, q))
            .collect()
    }

    fn walk_book(levels: &[(f64, f64)], target_qty: f64, mid_price: f64) -> WalkResult {
        let mut remaining = target_qty;
        let mut cost = 0.0;
        let mut filled = 0.0;
        let mut worst_price = 0.0;

        for &(price, qty) in levels {
            if remaining <= 0.0 {
                break;
            }
            let take = remaining.min(qty);
            cost += take * price;
            filled += take;
            remaining -= take;
            worst_price = price;
        }

        let vwap = if filled > 0.0 { cost / filled } else { 0.0 };
        let slippage_bps = if filled > 0.0 && mid_price > 0.0 {
            ((vwap - mid_price).abs() / mid_price) * 10_000.0
        } else {
            0.0
        };

        WalkResult {
            vwap,
            worst_price,
            filled_qty: filled,
            slippage_bps,
            fully_filled: remaining <= 1e-8 && filled > 0.0,
        }
    }

    fn format_utc_ms(ms: u64) -> String {
        let total_secs = (ms / 1000) % 86400;
        let hours = total_secs / 3600;
        let mins = (total_secs % 3600) / 60;
        let secs = total_secs % 60;
        let millis = ms % 1000;
        format!("{:02}:{:02}:{:02}.{:03}", hours, mins, secs, millis)
    }
}

impl eframe::App for OrderbookApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let now = ctx.input(|i| i.time);
        if now - self.last_sec_timestamp >= 1.0 {
            self.msgs_per_sec = self.msgs_this_sec;
            self.msgs_this_sec = 0;
            self.last_sec_timestamp = now;
        }

        // 1. Drain Depth WebSocket
        while let Some(event) = self.depth_receiver.try_recv() {
            if let WsEvent::Message(WsMessage::Text(text)) = event {
                self.depth_msg_count += 1;
                self.msgs_this_sec += 1;
                if !self.paused {
                    if let Ok(depth) = serde_json::from_str::<DepthUpdate>(&text) {
                        self.raw_bids.clear();
                        self.raw_asks.clear();
                        Self::update_levels(&mut self.raw_bids, depth.bids);
                        Self::update_levels(&mut self.raw_asks, depth.asks);
                    }
                }
            }
        }

        // 2. Drain AggTrade WebSocket
        while let Some(event) = self.trade_receiver.try_recv() {
            if let WsEvent::Message(WsMessage::Text(text)) = event {
                self.trade_msg_count += 1;
                self.msgs_this_sec += 1;
                if !self.paused {
                    if let Ok(trade) = serde_json::from_str::<AggTradeEvent>(&text) {
                        if let (Ok(price), Ok(qty)) =
                            (trade.price.parse::<f64>(), trade.qty.parse::<f64>())
                        {
                            self.trades.push_front(TradePrint {
                                price,
                                qty,
                                time_str: Self::format_utc_ms(trade.timestamp_ms),
                                is_buy_aggressor: !trade.is_buyer_maker,
                            });
                            if self.trades.len() > 50 {
                                self.trades.pop_back();
                            }
                        }
                    }
                }
            }
        }

        // Compute Aggregated Book & Microstructure Metrics
        let bids = self.aggregated_bids(); // Highest first
        let asks = self.aggregated_asks(); // Lowest first

        let best_bid = bids.first().copied().unwrap_or((0.0, 0.0));
        let best_ask = asks.first().copied().unwrap_or((0.0, 0.0));

        let mid_price = if best_bid.0 > 0.0 && best_ask.0 > 0.0 {
            (best_bid.0 + best_ask.0) / 2.0
        } else {
            0.0
        };

        let spread_usd = (best_ask.0 - best_bid.0).max(0.0);
        let spread_bps = if mid_price > 0.0 {
            (spread_usd / mid_price) * 10_000.0
        } else {
            0.0
        };

        // Volume-Weighted Microprice
        let top_vol_sum = best_bid.1 + best_ask.1;
        let microprice = if top_vol_sum > 0.0 && mid_price > 0.0 {
            (best_bid.1 * best_ask.0 + best_ask.1 * best_bid.0) / top_vol_sum
        } else {
            mid_price
        };
        let micro_delta_bps = if mid_price > 0.0 {
            ((microprice - mid_price) / mid_price) * 10_000.0
        } else {
            0.0
        };

        // Top 10 Level Order Book Imbalance (OBI)
        let bid_vol_10: f64 = bids.iter().take(10).map(|(_, q)| *q).sum();
        let ask_vol_10: f64 = asks.iter().take(10).map(|(_, q)| *q).sum();
        let total_vol_10 = bid_vol_10 + ask_vol_10;
        let obi_ratio = if total_vol_10 > 0.0 {
            bid_vol_10 / total_vol_10
        } else {
            0.5
        };

        // --- TOP BAR: Controls & Microstructure Header ---
        egui::TopBottomPanel::top("top_header")
            .frame(egui::Frame::side_top_panel(&ctx.style()).inner_margin(8.0))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading(
                        egui::RichText::new("⚡ WASM L2 MICROSTRUCTURE TERMINAL")
                            .color(egui::Color32::from_rgb(88, 166, 255))
                            .strong(),
                    );
                    ui.separator();

                    // Symbol Switcher
                    for sym in [
                        Symbol::BtcUsdt,
                        Symbol::EthUsdt,
                        Symbol::SolUsdt,
                        Symbol::BnbUsdt,
                    ] {
                        if ui
                            .selectable_label(self.symbol == sym, sym.label())
                            .clicked()
                        {
                            self.switch_symbol(sym);
                        }
                    }

                    ui.separator();
                    ui.label("Tick Group:");
                    for (cents, label) in [
                        (1, "$0.01"),
                        (10, "$0.10"),
                        (100, "$1.00"),
                        (500, "$5.00"),
                        (1000, "$10.0"),
                    ] {
                        if ui
                            .selectable_label(self.tick_bucket_cents == cents, label)
                            .clicked()
                        {
                            self.tick_bucket_cents = cents;
                        }
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let btn_text = if self.paused {
                            "▶ RESUME FEED"
                        } else {
                            "⏸ PAUSE FEED"
                        };
                        let btn_color = if self.paused {
                            egui::Color32::from_rgb(39, 174, 96)
                        } else {
                            egui::Color32::from_rgb(235, 87, 87)
                        };
                        if ui
                            .add(egui::Button::new(
                                egui::RichText::new(btn_text).color(btn_color).strong(),
                            ))
                            .clicked()
                        {
                            self.paused = !self.paused;
                        }
                    });
                });

                ui.add_space(4.0);
                ui.separator();
                ui.add_space(4.0);

                // Quantitative KPI Strip
                ui.horizontal(|ui| {
                    render_kpi(
                        ui,
                        "MID PRICE",
                        &format!("${:.2}", mid_price),
                        egui::Color32::WHITE,
                    );
                    ui.separator();
                    let mp_color = if micro_delta_bps >= 0.0 {
                        egui::Color32::from_rgb(39, 174, 96)
                    } else {
                        egui::Color32::from_rgb(235, 87, 87)
                    };
                    render_kpi(
                        ui,
                        "MICROPRICE (VOL-WEIGHTED)",
                        &format!("${:.2} ({:+.2} bps)", microprice, micro_delta_bps),
                        mp_color,
                    );
                    ui.separator();
                    render_kpi(
                        ui,
                        "SPREAD",
                        &format!("${:.2} ({:.2} bps)", spread_usd, spread_bps),
                        egui::Color32::from_rgb(242, 201, 76),
                    );
                    ui.separator();
                    let obi_pct = (obi_ratio - 0.5) * 200.0; // -100% to +100%
                    let obi_color = if obi_pct >= 0.0 {
                        egui::Color32::from_rgb(39, 174, 96)
                    } else {
                        egui::Color32::from_rgb(235, 87, 87)
                    };
                    render_kpi(
                        ui,
                        "L10 BOOK IMBALANCE",
                        &format!(
                            "{:+.1}% ({:.0}% B / {:.0}% A)",
                            obi_pct,
                            obi_ratio * 100.0,
                            (1.0 - obi_ratio) * 100.0
                        ),
                        obi_color,
                    );
                });
            });

        // --- BOTTOM BAR: Hardware & Network Telemetry ---
        egui::TopBottomPanel::bottom("bottom_telemetry")
            .frame(egui::Frame::side_top_panel(&ctx.style()).inner_margin(6.0))
            .show(ctx, |ui| {
                let dt = ctx.input(|i| i.stable_dt).max(0.0001);
                let fps = (1.0 / dt).min(240.0);

                ui.horizontal(|ui| {
                    let status_color = if self.paused {
                        egui::Color32::from_rgb(242, 201, 76)
                    } else {
                        egui::Color32::from_rgb(39, 174, 96)
                    };
                    ui.colored_label(
                        status_color,
                        if self.paused {
                            "● STREAM PAUSED"
                        } else {
                            "● LIVE BINANCE WSS"
                        },
                    );
                    ui.separator();
                    ui.monospace(format!("Throughput: {} msgs/s", self.msgs_per_sec));
                    ui.separator();
                    ui.monospace(format!("L2 Snapshots: {}", self.depth_msg_count));
                    ui.separator();
                    ui.monospace(format!("Trade Prints: {}", self.trade_msg_count));
                    ui.separator();
                    ui.monospace(format!("Render: {:.0} FPS ({:.2} ms)", fps, dt * 1000.0));
                });
            });

        // --- LEFT PANEL: L2 Order Book Ladder ---
        egui::SidePanel::left("l2_ladder_panel")
            .resizable(false)
            .exact_width(420.0)
            .show(ctx, |ui| {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.strong("L2 ORDER BOOK LADDER");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.monospace(
                            egui::RichText::new(format!("PRICE (USDT) | SIZE ({}) | CUM", self.symbol.base_asset()))
                                .size(11.0)
                                .color(egui::Color32::GRAY),
                        );
                    });
                });
                ui.separator();

                // Prepare top 14 levels with cumulative volume
                let ask_slice: Vec<(f64, f64)> = asks.iter().take(14).copied().collect();
                let bid_slice: Vec<(f64, f64)> = bids.iter().take(14).copied().collect();

                let mut ask_cum = Vec::with_capacity(ask_slice.len());
                let mut acc = 0.0;
                for &(p, q) in &ask_slice {
                    acc += q;
                    ask_cum.push((p, q, acc));
                }

                let mut bid_cum = Vec::with_capacity(bid_slice.len());
                let mut b_acc = 0.0;
                for &(p, q) in &bid_slice {
                    b_acc += q;
                    bid_cum.push((p, q, b_acc));
                }

                let max_cum = ask_cum
                    .last()
                    .map(|x| x.2)
                    .unwrap_or(0.0)
                    .max(bid_cum.last().map(|x| x.2).unwrap_or(0.0))
                    .max(0.0001);

                // Render Asks (Highest to Lowest so Best Ask is right above Spread)
                for &(price, qty, cum) in ask_cum.iter().rev() {
                    render_ladder_row(
                        ui,
                        price,
                        qty,
                        cum,
                        max_cum,
                        egui::Color32::from_rgba_unmultiplied(235, 87, 87, 38),
                        egui::Color32::from_rgba_unmultiplied(235, 87, 87, 90),
                        egui::Color32::from_rgb(248, 81, 73),
                    );
                }

                // OBI Bar & Spread Divider
                ui.add_space(4.0);
                render_obi_bar(ui, obi_ratio);
                ui.add_space(4.0);

                // Render Bids (Highest to Lowest so Best Bid is right below Spread)
                for &(price, qty, cum) in &bid_cum {
                    render_ladder_row(
                        ui,
                        price,
                        qty,
                        cum,
                        max_cum,
                        egui::Color32::from_rgba_unmultiplied(39, 174, 96, 38),
                        egui::Color32::from_rgba_unmultiplied(39, 174, 96, 90),
                        egui::Color32::from_rgb(46, 160, 67),
                    );
                }
            });

        // --- RIGHT PANEL: Live Time & Sales Tape ---
        egui::SidePanel::right("tape_panel")
            .resizable(false)
            .exact_width(340.0)
            .show(ctx, |ui| {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.strong("TIME & SALES (LIVE TAPE)");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(20.0);
                        ui.monospace(
                            egui::RichText::new("TIME (UTC) | PRICE | SIZE")
                                .size(11.0)
                                .color(egui::Color32::GRAY),
                        );
                    });
                });
                ui.separator();

                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for print in &self.trades {
                            let color = if print.is_buy_aggressor {
                                egui::Color32::from_rgb(46, 160, 67)
                            } else {
                                egui::Color32::from_rgb(248, 81, 73)
                            };
                            let side_tag = if print.is_buy_aggressor { "BUY " } else { "SELL" };

                            ui.horizontal(|ui| {
                                ui.monospace(
                                    egui::RichText::new(&print.time_str)
                                        .size(12.0)
                                        .color(egui::Color32::GRAY),
                                );
                                ui.monospace(
                                    egui::RichText::new(side_tag).size(11.0).color(color).strong(),
                                );
                                ui.monospace(
                                    egui::RichText::new(format!("{:.2}", print.price))
                                        .size(12.5)
                                        .color(color),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.monospace(
                                            egui::RichText::new(format!("{:.4}", print.qty))
                                                .size(12.5)
                                                .color(egui::Color32::LIGHT_GRAY),
                                        );
                                    },
                                );
                            });
                        }
                    });
            });

        // --- CENTER PANEL: Cumulative Depth Chart + Market Impact Simulator ---
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.strong("CUMULATIVE LIQUIDITY DEPTH WALLS (L2 STEP CHART)");
            ui.add_space(4.0);

            // Allocate upper 62% of center panel to the WebGL depth chart
            let chart_height = (ui.available_height() * 0.60).max(180.0);
            render_depth_chart(ui, &bids, &asks, mid_price, chart_height);

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(4.0);

            // Pre-Trade Market Impact & Slippage Simulator
            ui.horizontal(|ui| {
                ui.strong("PRE-TRADE MARKET IMPACT & SLIPPAGE SIMULATOR");
                ui.separator();
                ui.label(format!("Sweep Order Size ({}):", self.symbol.base_asset()));
                ui.add(
                    egui::Slider::new(&mut self.sim_order_qty, 0.01..=50.0)
                        .logarithmic(true)
                        .text(self.symbol.base_asset()),
                );
            });

            ui.add_space(6.0);

            let buy_walk = Self::walk_book(&asks, self.sim_order_qty, mid_price);
            let sell_walk = Self::walk_book(&bids, self.sim_order_qty, mid_price);

            ui.columns(2, |cols| {
                render_impact_card(
                    &mut cols[0],
                    "MARKET BUY (SWEEP ASKS)",
                    &buy_walk,
                    self.symbol.base_asset(),
                    egui::Color32::from_rgb(46, 160, 67),
                );
                render_impact_card(
                    &mut cols[1],
                    "MARKET SELL (SWEEP BIDS)",
                    &sell_walk,
                    self.symbol.base_asset(),
                    egui::Color32::from_rgb(248, 81, 73),
                );
            });
        });

        ctx.request_repaint();
    }
}

fn render_kpi(ui: &mut egui::Ui, title: &str, value: &str, color: egui::Color32) {
    ui.vertical(|ui| {
        ui.label(egui::RichText::new(title).size(10.5).color(egui::Color32::GRAY));
        ui.monospace(egui::RichText::new(value).size(14.5).color(color).strong());
    });
}

fn render_obi_bar(ui: &mut egui::Ui, obi_ratio: f64) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 14.0), egui::Sense::hover());

    // Ask background
    ui.painter().rect_filled(
        rect,
        3.0,
        egui::Color32::from_rgb(180, 50, 50),
    );

    // Bid fill
    let bid_w = (rect.width() * (obi_ratio as f32)).clamp(0.0, rect.width());
    let bid_rect = egui::Rect::from_min_size(rect.min, egui::vec2(bid_w, rect.height()));
    ui.painter().rect_filled(
        bid_rect,
        3.0,
        egui::Color32::from_rgb(35, 145, 75),
    );

    // Midline marker
    ui.painter().line_segment(
        [
            egui::pos2(rect.center().x, rect.top()),
            egui::pos2(rect.center().x, rect.bottom()),
        ],
        egui::Stroke::new(1.5, egui::Color32::WHITE),
    );

    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        format!("IMBALANCE: {:.0}% BID / {:.0}% ASK", obi_ratio * 100.0, (1.0 - obi_ratio) * 100.0),
        egui::FontId::monospace(10.0),
        egui::Color32::WHITE,
    );
}

#[allow(clippy::too_many_arguments)]
fn render_ladder_row(
    ui: &mut egui::Ui,
    price: f64,
    qty: f64,
    cum_qty: f64,
    max_cum: f64,
    cum_bar_color: egui::Color32,
    size_bar_color: egui::Color32,
    text_color: egui::Color32,
) {
    let row_width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(row_width, 18.0), egui::Sense::hover());

    // 1. Cumulative Volume Shading (Full background bar)
    let cum_width = (rect.width() * (cum_qty / max_cum) as f32).clamp(0.0, rect.width());
    let cum_rect = egui::Rect::from_min_size(
        egui::pos2(rect.right() - cum_width, rect.top()),
        egui::vec2(cum_width, rect.height()),
    );
    ui.painter().rect_filled(cum_rect, 0.0, cum_bar_color);

    // 2. Individual Level Volume Bar (Accent strip at bottom of row)
    let size_width = (rect.width() * (qty / max_cum) as f32).clamp(0.0, rect.width());
    let size_rect = egui::Rect::from_min_size(
        egui::pos2(rect.right() - size_width, rect.bottom() - 3.0),
        egui::vec2(size_width, 3.0),
    );
    ui.painter().rect_filled(size_rect, 0.0, size_bar_color);

    // 3. Columns: Price | Size | Cumulative Total
    ui.painter().text(
        egui::pos2(rect.left() + 10.0, rect.top() + 2.0), // Shifted in from 6.0
        egui::Align2::LEFT_TOP,
        format!("{:.2}", price),
        egui::FontId::monospace(12.5),
        text_color,
    );
    ui.painter().text(
        egui::pos2(rect.center().x + 25.0, rect.top() + 2.0),
        egui::Align2::RIGHT_TOP,
        format!("{:.4}", qty),
        egui::FontId::monospace(12.5),
        egui::Color32::LIGHT_GRAY,
    );
    ui.painter().text(
        egui::pos2(rect.right() - 14.0, rect.top() + 2.0), // Shifted in to prevent hugging the divider
        egui::Align2::RIGHT_TOP,
        format!("{:.3}", cum_qty),
        egui::FontId::monospace(12.0),
        egui::Color32::GRAY,
    );
}

fn render_depth_chart(
    ui: &mut egui::Ui,
    bids: &[(f64, f64)],
    asks: &[(f64, f64)],
    mid_price: f64,
    height: f32,
) {
    let width = ui.available_width();
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let painter = ui.painter_at(rect);

    painter.rect_filled(rect, 4.0, egui::Color32::from_rgb(9, 12, 17));
    painter.rect_stroke(
        rect,
        4.0,
        egui::Stroke::new(1.0, egui::Color32::from_rgb(35, 40, 50)),
    );

    if bids.is_empty() || asks.is_empty() || mid_price <= 0.0 {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Awaiting L2 Depth Stream...",
            egui::FontId::monospace(14.0),
            egui::Color32::GRAY,
        );
        return;
    }

    // Build cumulative curves
    let mut bid_curve = Vec::with_capacity(bids.len());
    let mut b_sum = 0.0;
    for &(p, q) in bids {
        b_sum += q;
        bid_curve.push((p, b_sum));
    }

    let mut ask_curve = Vec::with_capacity(asks.len());
    let mut a_sum = 0.0;
    for &(p, q) in asks {
        a_sum += q;
        ask_curve.push((p, a_sum));
    }

    let min_price = bid_curve.last().map(|x| x.0).unwrap_or(mid_price * 0.999);
    let max_price = ask_curve.last().map(|x| x.0).unwrap_or(mid_price * 1.001);
    let price_span = (max_price - min_price).max(0.01);
    let max_vol = b_sum.max(a_sum).max(0.001);

    let pad_bottom = 22.0;
    let plot_rect = egui::Rect::from_min_max(
        rect.min + egui::vec2(8.0, 12.0),
        rect.max - egui::vec2(8.0, pad_bottom),
    );

    let price_to_x = |p: f64| -> f32 {
        let ratio = ((p - min_price) / price_span).clamp(0.0, 1.0) as f32;
        plot_rect.left() + ratio * plot_rect.width()
    };

    let vol_to_y = |v: f64| -> f32 {
        let ratio = (v / max_vol).clamp(0.0, 1.0) as f32;
        plot_rect.bottom() - ratio * plot_rect.height()
    };

    // Draw Stepped Bid Wall (Green)
    let bid_fill = egui::Color32::from_rgba_unmultiplied(46, 160, 67, 55);
    let bid_stroke = egui::Stroke::new(2.0, egui::Color32::from_rgb(46, 160, 67));
    let mut prev_x = price_to_x(bid_curve[0].0);
    let mut prev_y = plot_rect.bottom();

    for &(p, cum_v) in &bid_curve {
        let x = price_to_x(p);
        let y = vol_to_y(cum_v);

        // Fill vertical column from step to bottom
        let fill_rect = egui::Rect::from_min_max(
            egui::pos2(x.min(prev_x), y),
            egui::pos2(x.max(prev_x), plot_rect.bottom()),
        );
        painter.rect_filled(fill_rect, 0.0, bid_fill);

        // Step lines
        painter.line_segment([egui::pos2(prev_x, prev_y), egui::pos2(prev_x, y)], bid_stroke);
        painter.line_segment([egui::pos2(prev_x, y), egui::pos2(x, y)], bid_stroke);

        prev_x = x;
        prev_y = y;
    }

    // Draw Stepped Ask Wall (Red)
    let ask_fill = egui::Color32::from_rgba_unmultiplied(248, 81, 73, 55);
    let ask_stroke = egui::Stroke::new(2.0, egui::Color32::from_rgb(248, 81, 73));
    let mut prev_ax = price_to_x(ask_curve[0].0);
    let mut prev_ay = plot_rect.bottom();

    for &(p, cum_v) in &ask_curve {
        let x = price_to_x(p);
        let y = vol_to_y(cum_v);

        let fill_rect = egui::Rect::from_min_max(
            egui::pos2(prev_ax.min(x), y),
            egui::pos2(prev_ax.max(x), plot_rect.bottom()),
        );
        painter.rect_filled(fill_rect, 0.0, ask_fill);

        painter.line_segment([egui::pos2(prev_ax, prev_ay), egui::pos2(prev_ax, y)], ask_stroke);
        painter.line_segment([egui::pos2(prev_ax, y), egui::pos2(x, y)], ask_stroke);

        prev_ax = x;
        prev_ay = y;
    }

    // Mid-Price Center Line
    let mid_x = price_to_x(mid_price);
    painter.line_segment(
        [
            egui::pos2(mid_x, plot_rect.top()),
            egui::pos2(mid_x, plot_rect.bottom()),
        ],
        egui::Stroke::new(1.0, egui::Color32::from_gray(110)),
    );

    // Axis Labels
    painter.text(
        egui::pos2(plot_rect.left(), rect.bottom() - 4.0),
        egui::Align2::LEFT_BOTTOM,
        format!("${:.2}", min_price),
        egui::FontId::monospace(11.0),
        egui::Color32::GRAY,
    );
    painter.text(
        egui::pos2(mid_x, rect.bottom() - 4.0),
        egui::Align2::CENTER_BOTTOM,
        format!("Mid: ${:.2}", mid_price),
        egui::FontId::monospace(11.0),
        egui::Color32::LIGHT_GRAY,
    );
    painter.text(
        egui::pos2(plot_rect.right(), rect.bottom() - 4.0),
        egui::Align2::RIGHT_BOTTOM,
        format!("${:.2}", max_price),
        egui::FontId::monospace(11.0),
        egui::Color32::GRAY,
    );

    // Interactive Hover Crosshair Inspection
    if let Some(pointer) = response.hover_pos() {
        if plot_rect.contains(pointer) {
            let hovered_ratio = (pointer.x - plot_rect.left()) / plot_rect.width();
            let hovered_price = min_price + (hovered_ratio as f64) * price_span;

            let cum_at_hover = if hovered_price <= mid_price {
                bid_curve
                    .iter()
                    .rfind(|&&(p, _)| p >= hovered_price)
                    .map(|x| x.1)
                    .unwrap_or(0.0)
            } else {
                ask_curve
                    .iter()
                    .rfind(|&&(p, _)| p <= hovered_price)
                    .map(|x| x.1)
                    .unwrap_or(0.0)
            };

            painter.line_segment(
                [
                    egui::pos2(pointer.x, plot_rect.top()),
                    egui::pos2(pointer.x, plot_rect.bottom()),
                ],
                egui::Stroke::new(1.0, egui::Color32::WHITE),
            );

            painter.text(
                egui::pos2(pointer.x, plot_rect.top() + 6.0),
                egui::Align2::CENTER_TOP,
                format!("Price: ${:.2} | Cum Depth: {:.4}", hovered_price, cum_at_hover),
                egui::FontId::monospace(12.0),
                egui::Color32::WHITE,
            );
        }
    }
}

fn render_impact_card(
    ui: &mut egui::Ui,
    title: &str,
    walk: &WalkResult,
    asset: &str,
    accent: egui::Color32,
) {
    egui::Frame::group(ui.style())
        .inner_margin(8.0)
        .show(ui, |ui| {
            ui.colored_label(accent, egui::RichText::new(title).strong());
            ui.separator();
            ui.monospace(format!("Est. Fill VWAP:  ${:.2}", walk.vwap));
            ui.monospace(format!("Worst Price:     ${:.2}", walk.worst_price));
            ui.monospace(format!("Slippage Impact: {:.2} bps", walk.slippage_bps));
            let fill_status = if walk.fully_filled {
                format!("Filled: {:.4} {} (100%)", walk.filled_qty, asset)
            } else {
                format!("Partial Fill: {:.4} {} (Book Swept!)", walk.filled_qty, asset)
            };
            ui.monospace(fill_status);
        });
}