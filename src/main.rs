mod book;
mod feed;
mod symbol;
mod trades;
mod ui;

use book::OrderBook;
use eframe::egui;
use feed::{Feed, FeedEvent, Health};
use symbol::Symbol;
use trades::TradeTape;
use ui::{ACCENT_GOLD, ASK_TEXT, BG_PRIMARY, BID_TEXT};

fn main() -> eframe::Result<()> {
    #[cfg(target_arch = "wasm32")]
    {
        eframe::WebLogger::init(log::LevelFilter::Info).ok();
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

struct OrderbookApp {
    symbol: Symbol,
    feed: Feed,
    event_buf: Vec<FeedEvent>,

    book: OrderBook,
    tape: TradeTape,

    // User Controls
    bucket_units: i64,
    paused: bool,
    sim_order_qty: f64,
    taker_fee_bps: f64,
    whale_only_tape: bool,

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
        visuals.panel_fill = BG_PRIMARY;
        visuals.window_fill = BG_PRIMARY;
        cc.egui_ctx.set_visuals(visuals);

        let symbol = Symbol::BtcUsdt;
        Self {
            symbol,
            feed: Feed::new(&cc.egui_ctx, symbol),
            event_buf: Vec::with_capacity(64),
            book: OrderBook::new(symbol.price_scale()),
            tape: TradeTape::default(),
            bucket_units: 1,
            paused: false,
            sim_order_qty: symbol.default_sim_qty(),
            taker_fee_bps: 10.0, // Standard 0.10% Binance spot taker fee
            whale_only_tape: false,
            depth_msg_count: 0,
            trade_msg_count: 0,
            msgs_this_sec: 0,
            msgs_per_sec: 0,
            last_sec_timestamp: 0.0,
        }
    }

    fn switch_symbol(&mut self, new_symbol: Symbol, now: f64) {
        if self.symbol == new_symbol {
            return;
        }
        self.symbol = new_symbol;
        self.book.set_scale(new_symbol.price_scale());
        self.tape.clear();
        self.bucket_units = 1;
        self.sim_order_qty = new_symbol.default_sim_qty();
        self.feed.set_symbol(new_symbol, now);
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

        // 1. Poll Combined WebSocket Feed
        self.event_buf.clear();
        self.feed.poll(now, &mut self.event_buf);

        for ev in self.event_buf.drain(..) {
            self.msgs_this_sec += 1;
            match ev {
                FeedEvent::Depth(depth) => {
                    self.depth_msg_count += 1;
                    if !self.paused {
                        self.book.apply_snapshot(&depth.bids, &depth.asks);
                    }
                }
                FeedEvent::Trade(trade) => {
                    self.trade_msg_count += 1;
                    if !self.paused {
                        self.tape.push(trade);
                    }
                }
            }
        }

        let metrics = self.book.metrics();
        let ladder = self.book.ladder(self.bucket_units);
        let whale_usd = self.symbol.default_whale_usd();
        let flow = self.tape.stats(whale_usd);

        // --- TOP BAR ---
        egui::TopBottomPanel::top("top_header")
            .frame(egui::Frame::side_top_panel(&ctx.style()).inner_margin(8.0))
            .show(ctx, |ui| {
                let action = ui::header::render_header(
                    ui,
                    self.symbol,
                    self.bucket_units,
                    self.paused,
                    &metrics,
                    &flow,
                    self.tape.cvd,
                );
                if let Some(sym) = action.new_symbol {
                    self.switch_symbol(sym, now);
                }
                if let Some(b) = action.new_bucket {
                    self.bucket_units = b;
                }
                if action.toggle_pause {
                    self.paused = !self.paused;
                }
                if action.clear_tape {
                    self.tape.clear();
                }
            });

        // --- BOTTOM TELEMETRY BAR ---
        egui::TopBottomPanel::bottom("bottom_telemetry")
            .frame(egui::Frame::side_top_panel(&ctx.style()).inner_margin(6.0))
            .show(ctx, |ui| {
                let dt = ctx.input(|i| i.stable_dt).max(0.0001);
                let fps = (1.0 / dt).min(240.0);
                let health = self.feed.health(now);

                ui.horizontal(|ui| {
                    let (status_text, status_color) = if self.paused {
                        ("● STREAM PAUSED".to_string(), ACCENT_GOLD)
                    } else {
                        match health {
                            Health::Live => ("● LIVE BINANCE WSS".to_string(), BID_TEXT),
                            Health::Connecting => ("◌ CONNECTING...".to_string(), ACCENT_GOLD),
                            Health::Stale(s) => (format!("● STALE ({s:.1}s)"), ASK_TEXT),
                            Health::Reconnecting { attempt, in_secs } => (
                                format!("◌ RETRY #{attempt} ({in_secs:.1}s)"),
                                ASK_TEXT,
                            ),
                        }
                    };
                    ui.colored_label(status_color, status_text);

                    if ui.small_button("↻").on_hover_text("Reconnect stream").clicked() {
                        self.feed.force_reconnect(now);
                    }

                    ui.separator();
                    ui.monospace(format!("{} msgs/s", self.msgs_per_sec));
                    ui.separator();
                    ui.monospace(format!("L2: {}", self.depth_msg_count));
                    ui.separator();
                    ui.monospace(format!("Trades: {}", self.trade_msg_count));
                    ui.separator();
                    ui.monospace(format!("Reconnects: {}", self.feed.reconnects));
                    ui.separator();
                    ui.monospace(format!("Render: {:.0} FPS ({:.2} ms)", fps, dt * 1000.0));
                });
            });

        // --- LEFT PANEL: L2 LADDER ---
        egui::SidePanel::left("l2_ladder_panel")
            .resizable(false)
            .exact_width(420.0)
            .show(ctx, |ui| {
                if let Some(sweep_qty) = ui::ladder::render_ladder(
                    ui,
                    self.symbol,
                    &ladder,
                    metrics.obi,
                    metrics.spread_bps,
                ) {
                    self.sim_order_qty = sweep_qty;
                }
            });

        // --- RIGHT PANEL: TIME & SALES TAPE ---
        egui::SidePanel::right("tape_panel")
            .resizable(false)
            .exact_width(350.0)
            .show(ctx, |ui| {
                ui::tape::render_tape(
                    ui,
                    self.symbol,
                    &self.tape,
                    &flow,
                    &mut self.whale_only_tape,
                    whale_usd,
                );
            });

        // --- CENTER PANEL: DEPTH CHART + SIMULATOR ---
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.strong("CUMULATIVE LIQUIDITY DEPTH WALLS (L2 STEP CHART)");
            ui.add_space(4.0);

            let chart_height = (ui.available_height() * 0.58).max(180.0);
            ui::chart::render_depth_chart(ui, self.symbol, &ladder, metrics.mid, chart_height);

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(4.0);

            ui::simulator::render_simulator(
                ui,
                self.symbol,
                &self.book,
                metrics.mid,
                &mut self.sim_order_qty,
                &mut self.taker_fee_bps,
            );
        });

        ctx.request_repaint();
    }
}