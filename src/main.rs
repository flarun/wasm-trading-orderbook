use eframe::egui;
use ewebsock::{WsEvent, WsMessage, WsReceiver, WsSender};

fn main() -> eframe::Result<()> {
    #[cfg(target_arch = "wasm32")]
    {
        // WebAssembly specific initialization
        eframe::WebLogger::init(log::LevelFilter::Debug).ok();
        let web_options = eframe::WebOptions::default();
        wasm_bindgen_futures::spawn_local(async {
            eframe::WebRunner::new()
                .start(
                    "the_canvas_id",
                    web_options,
                    Box::new(|cc| Box::new(OrderbookApp::new(cc))),
                )+
                .await
                .expect("Failed to start eframe");
        });
        return Ok(());
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        // Native desktop initialization
        let options = eframe::NativeOptions::default();
        eframe::run_native(
            "Wasm Trading Orderbook",
            options,
            Box::new(|cc| Box::new(OrderbookApp::new(cc))),
        )
    }
}

struct OrderbookApp {
    sender: WsSender,
    receiver: WsReceiver,
    events: Vec<String>,
}

impl OrderbookApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Force Dark Mode to look like a proper trading terminal
        cc.egui_ctx.set_visuals(egui::Visuals::dark());

        // Connect to Binance BTC/USDT 100ms Order Book Depth Stream
        let url = "wss://stream.binance.com:9443/ws/btcusdt@depth@100ms";
        
        let (sender, receiver) = ewebsock::connect(url, ewebsock::Options::default())
            .expect("Failed to connect to WebSocket");

        Self {
            sender,
            receiver,
            events: Vec::new(),
        }
    }
}

impl eframe::App for OrderbookApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 1. Process incoming WebSocket messages
        while let Some(event) = self.receiver.try_recv() {
            match event {
                WsEvent::Opened => self.events.push("Connected to Binance L2 Stream.".to_string()),
                WsEvent::Message(msg) => {
                    if let WsMessage::Text(text) = msg {
                        // For now, just store the first 100 chars so we don't crash the UI
                        let snippet = text.chars().take(100).collect::<String>();
                        self.events.push(format!("Received: {}...", snippet));
                        
                        // Keep event log small
                        if self.events.len() > 20 {
                            self.events.remove(0);
                        }
                    }
                }
                WsEvent::Error(err) => self.events.push(format!("Error: {}", err)),
                WsEvent::Closed => self.events.push("Connection closed.".to_string()),
            }
        }

        // 2. Render the UI
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Live Order Book (BTC/USDT)");
            ui.separator();

            ui.label("WebSocket Event Log:");
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for event in &self.events {
                        ui.label(event);
                    }
                });
        });

        // 3. Request a repaint continuously so the UI updates as packets arrive
        ctx.request_repaint();
    }
}