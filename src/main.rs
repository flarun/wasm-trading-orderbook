use eframe::egui;
use ewebsock::{WsEvent, WsMessage, WsReceiver, WsSender};
use serde::Deserialize;
use std::collections::BTreeMap;

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

// Fix 1: The @depth20 stream uses "bids" and "asks", not "b" and "a"
#[derive(Deserialize)]
struct DepthUpdate {
    bids: Vec<[String; 2]>, 
    asks: Vec<[String; 2]>, 
}

struct OrderbookApp {
    _sender: WsSender,
    receiver: WsReceiver,
    bids: BTreeMap<i64, f64>,
    asks: BTreeMap<i64, f64>,
    message_count: u64,
}

impl OrderbookApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_visuals(egui::Visuals::dark());

        // We request the top 20 levels every 100ms
        let url = "wss://stream.binance.com:9443/ws/btcusdt@depth20@100ms";
        let (sender, receiver) = ewebsock::connect(url, ewebsock::Options::default())
            .expect("Failed to connect to WebSocket");

        Self {
            _sender: sender,
            receiver,
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
            message_count: 0,
        }
    }

    fn update_levels(
        book: &mut BTreeMap<i64, f64>,
        raw_levels: Vec<[String; 2]>,
    ) {
        for entry in raw_levels {
            if let (Ok(p_str), Ok(q)) = (entry[0].parse::<f64>(), entry[1].parse::<f64>()) {
                let price_key = (p_str * 100.0).round() as i64;
                if q > 0.0 {
                    book.insert(price_key, q);
                }
            }
        }
    }
}

impl eframe::App for OrderbookApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 1. Process WebSocket updates
        while let Some(event) = self.receiver.try_recv() {
            if let WsEvent::Message(WsMessage::Text(text)) = event {
                if let Ok(depth) = serde_json::from_str::<DepthUpdate>(&text) {
                    // Fix 1b: Clear the old snapshot before inserting the new 20 levels
                    self.bids.clear();
                    self.asks.clear();

                    Self::update_levels(&mut self.bids, depth.bids);
                    Self::update_levels(&mut self.asks, depth.asks);
                    self.message_count += 1;
                }
            }
        }

        // 2. Render Orderbook Interface
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("BTC/USDT L2 Market Depth");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(format!("Updates: {}", self.message_count));
                });
            });
            ui.separator();

            let max_bid_qty = self.bids.values().copied().fold(0.0, f64::max);
            let max_ask_qty = self.asks.values().copied().fold(0.0, f64::max);
            let max_qty = max_bid_qty.max(max_ask_qty).max(0.0001);

            // Asks Table
            ui.label(egui::RichText::new("ASKS").color(egui::Color32::from_rgb(235, 87, 87)).strong());
            let ask_levels: Vec<_> = self.asks.iter().take(12).collect();
            for (price_int, qty) in ask_levels.iter().rev() {
                let display_price = (**price_int as f64) / 100.0;
                render_level_row(ui, display_price, **qty, max_qty, egui::Color32::from_rgba_unmultiplied(235, 87, 87, 40), egui::Color32::from_rgb(235, 87, 87));
            }

            // Spread Indicator
            let best_bid_int = self.bids.keys().next_back().copied().unwrap_or(0);
            let best_ask_int = self.asks.keys().next().copied().unwrap_or(0);
            
            let best_bid = (best_bid_int as f64) / 100.0;
            let best_ask = (best_ask_int as f64) / 100.0;
            let spread = (best_ask - best_bid).max(0.0);

            ui.separator();
            ui.horizontal(|ui| {
                ui.label(format!("Spread: {:.2}", spread));
                ui.label(format!("Mid: {:.2}", (best_bid + best_ask) / 2.0));
            });
            ui.separator();

            // Bids Table
            ui.label(egui::RichText::new("BIDS").color(egui::Color32::from_rgb(39, 174, 96)).strong());
            let bid_levels: Vec<_> = self.bids.iter().rev().take(12).collect();
            for (price_int, qty) in bid_levels {
                let display_price = (*price_int as f64) / 100.0;
                render_level_row(ui, display_price, *qty, max_qty, egui::Color32::from_rgba_unmultiplied(39, 174, 96, 40), egui::Color32::from_rgb(39, 174, 96));
            }
        });

        ctx.request_repaint();
    }
}

fn render_level_row(
    ui: &mut egui::Ui,
    price: f64,
    qty: f64,
    max_qty: f64,
    bar_color: egui::Color32,
    text_color: egui::Color32,
) {
    // Fix 2: Hardcode the width to exactly 400 pixels to prevent layout jitter loop
    let row_width = 400.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(row_width, 18.0), egui::Sense::hover());
    
    let fill_width = (rect.width() * (qty / max_qty) as f32).clamp(0.0, rect.width());
    let bar_rect = egui::Rect::from_min_size(
        egui::pos2(rect.right() - fill_width, rect.top()),
        egui::vec2(fill_width, rect.height()),
    );
    ui.painter().rect_filled(bar_rect, 0.0, bar_color);

    ui.painter().text(
        egui::pos2(rect.left() + 10.0, rect.top() + 2.0),
        egui::Align2::LEFT_TOP,
        format!("{:.2}", price),
        egui::FontId::monospace(13.0),
        text_color,
    );
    ui.painter().text(
        egui::pos2(rect.right() - 10.0, rect.top() + 2.0),
        egui::Align2::RIGHT_TOP,
        format!("{:.4}", qty),
        egui::FontId::monospace(13.0),
        egui::Color32::LIGHT_GRAY,
    );
}