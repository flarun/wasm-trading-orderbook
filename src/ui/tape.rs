use crate::symbol::Symbol;
use crate::trades::{FlowStats, TradeTape};
use crate::ui::{ACCENT_GOLD, ASK_TEXT, BID_TEXT};
use egui::{Color32, RichText, Ui};

pub fn render_tape(
    ui: &mut Ui,
    symbol: Symbol,
    tape: &TradeTape,
    flow: &FlowStats,
    whale_only: &mut bool,
    whale_usd: f64,
) {
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.strong("TIME & SALES");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.checkbox(whale_only, format!("≥ ${:.0}k", whale_usd / 1000.0));
        });
    });

    // 60s Rolling Buy/Sell Aggressor Pressure Bar
    let buy_ratio = flow.buy_ratio();
    let (bar_rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 12.0), egui::Sense::hover());
    ui.painter()
        .rect_filled(bar_rect, 2.0, Color32::from_rgb(180, 50, 50));
    let bid_w = (bar_rect.width() * buy_ratio as f32).clamp(0.0, bar_rect.width());
    ui.painter().rect_filled(
        egui::Rect::from_min_size(bar_rect.min, egui::vec2(bid_w, bar_rect.height())),
        2.0,
        Color32::from_rgb(35, 145, 75),
    );
    ui.painter().text(
        bar_rect.center(),
        egui::Align2::CENTER_CENTER,
        format!(
            "60s TAKER FLOW: {:.0}% BUY / {:.0}% SELL ({} whales)",
            buy_ratio * 100.0,
            (1.0 - buy_ratio) * 100.0,
            flow.large_trades
        ),
        egui::FontId::monospace(9.5),
        Color32::WHITE,
    );

    ui.separator();

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for trade in tape.recent(120) {
                let is_whale = trade.notional() >= whale_usd;
                if *whale_only && !is_whale {
                    continue;
                }

                let color = if trade.is_buy { BID_TEXT } else { ASK_TEXT };
                let side_tag = if trade.is_buy { "BUY " } else { "SELL" };

                ui.horizontal(|ui| {
                    ui.monospace(
                        RichText::new(trade.format_utc())
                            .size(11.5)
                            .color(Color32::GRAY),
                    );
                    ui.monospace(RichText::new(side_tag).size(11.0).color(color).strong());
                    ui.monospace(
                        RichText::new(symbol.format_price(trade.price))
                            .size(12.0)
                            .color(color),
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let qty_color = if is_whale {
                            ACCENT_GOLD
                        } else {
                            Color32::LIGHT_GRAY
                        };
                        ui.monospace(
                            RichText::new(symbol.format_qty(trade.qty))
                                .size(12.0)
                                .color(qty_color),
                        );
                    });
                });
            }
        });
}