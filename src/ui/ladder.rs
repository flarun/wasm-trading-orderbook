use crate::book::Ladder;
use crate::symbol::Symbol;
use crate::ui::{
    ACCENT_GOLD, ASK_FILL_LIGHT, ASK_FILL_STRONG, ASK_TEXT, BID_FILL_LIGHT, BID_FILL_STRONG,
    BID_TEXT,
};
use egui::{Align2, Color32, FontId, RichText, Sense, Ui};

/// Renders the L2 order book ladder. Returns `Some(cumulative_qty)` if the user
/// clicks a price level to sweep up to that level in the simulator.
pub fn render_ladder(
    ui: &mut Ui,
    symbol: Symbol,
    ladder: &Ladder,
    obi_ratio: f64,
    spread_bps: f64,
) -> Option<f64> {
    let mut clicked_sweep_qty = None;

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.strong("L2 ORDER BOOK LADDER");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.monospace(
                RichText::new(format!(
                    "PRICE | SIZE ({}) | CUM",
                    symbol.base_asset()
                ))
                .size(11.0)
                .color(Color32::GRAY),
            );
        });
    });
    ui.separator();

    let rows_per_side = 15;
    let ask_slice: Vec<_> = ladder.asks.iter().take(rows_per_side).copied().collect();
    let bid_slice: Vec<_> = ladder.bids.iter().take(rows_per_side).copied().collect();

    let mut ask_cum = Vec::with_capacity(ask_slice.len());
    let mut acc = 0.0;
    for lvl in &ask_slice {
        acc += lvl.qty;
        ask_cum.push((lvl.price, lvl.qty, acc));
    }

    let mut bid_cum = Vec::with_capacity(bid_slice.len());
    let mut b_acc = 0.0;
    for lvl in &bid_slice {
        b_acc += lvl.qty;
        bid_cum.push((lvl.price, lvl.qty, b_acc));
    }

    let max_cum = ask_cum
        .last()
        .map(|x| x.2)
        .unwrap_or(0.0)
        .max(bid_cum.last().map(|x| x.2).unwrap_or(0.0))
        .max(1e-6);

    // Detect outlier liquidity walls (> 2.5x average visible level size)
    let avg_size = (ask_cum.iter().map(|x| x.1).sum::<f64>()
        + bid_cum.iter().map(|x| x.1).sum::<f64>())
        / (ask_cum.len() + bid_cum.len()).max(1) as f64;
    let wall_threshold = avg_size * 2.5;

    // Asks (Highest to lowest so Best Ask sits directly above spread)
    for &(price, qty, cum) in ask_cum.iter().rev() {
        let is_wall = qty >= wall_threshold && avg_size > 0.0;
        if render_row(
            ui,
            symbol,
            price,
            qty,
            cum,
            max_cum,
            is_wall,
            ASK_FILL_LIGHT,
            ASK_FILL_STRONG,
            ASK_TEXT,
        ) {
            clicked_sweep_qty = Some(cum);
        }
    }

    ui.add_space(4.0);
    render_obi_bar(ui, obi_ratio, spread_bps);
    ui.add_space(4.0);

    // Bids (Highest to lowest so Best Bid sits directly below spread)
    for &(price, qty, cum) in &bid_cum {
        let is_wall = qty >= wall_threshold && avg_size > 0.0;
        if render_row(
            ui,
            symbol,
            price,
            qty,
            cum,
            max_cum,
            is_wall,
            BID_FILL_LIGHT,
            BID_FILL_STRONG,
            BID_TEXT,
        ) {
            clicked_sweep_qty = Some(cum);
        }
    }

    ui.add_space(4.0);
    ui.label(
        RichText::new("Tip: Click any price level to load its cumulative sweep size into the simulator.")
            .size(10.5)
            .color(Color32::GRAY),
    );

    clicked_sweep_qty
}

fn render_obi_bar(ui: &mut Ui, obi_ratio: f64, spread_bps: f64) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 16.0), Sense::hover());

    ui.painter()
        .rect_filled(rect, 3.0, Color32::from_rgb(180, 50, 50));

    let bid_w = (rect.width() * (obi_ratio as f32)).clamp(0.0, rect.width());
    let bid_rect = egui::Rect::from_min_size(rect.min, egui::vec2(bid_w, rect.height()));
    ui.painter()
        .rect_filled(bid_rect, 3.0, Color32::from_rgb(35, 145, 75));

    ui.painter().line_segment(
        [
            egui::pos2(rect.center().x, rect.top()),
            egui::pos2(rect.center().x, rect.bottom()),
        ],
        egui::Stroke::new(1.5, Color32::WHITE),
    );

    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        format!(
            "OBI {:.0}% / {:.0}%  •  SPREAD {:.2} bps",
            obi_ratio * 100.0,
            (1.0 - obi_ratio) * 100.0,
            spread_bps
        ),
        FontId::monospace(10.5),
        Color32::WHITE,
    );
}

#[allow(clippy::too_many_arguments)]
fn render_row(
    ui: &mut Ui,
    symbol: Symbol,
    price: f64,
    qty: f64,
    cum_qty: f64,
    max_cum: f64,
    is_wall: bool,
    cum_color: Color32,
    size_color: Color32,
    text_color: Color32,
) -> bool {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 18.5), Sense::click());

    if response.hovered() {
        ui.painter()
            .rect_filled(rect, 0.0, Color32::from_rgba_unmultiplied(255, 255, 255, 18));
    }

    // 1. Cumulative background bar
    let cum_w = (rect.width() * (cum_qty / max_cum) as f32).clamp(0.0, rect.width());
    let cum_rect = egui::Rect::from_min_size(
        egui::pos2(rect.right() - cum_w, rect.top()),
        egui::vec2(cum_w, rect.height()),
    );
    ui.painter().rect_filled(cum_rect, 0.0, cum_color);

    // 2. Level size bar
    let size_w = (rect.width() * (qty / max_cum) as f32).clamp(0.0, rect.width());
    let size_rect = egui::Rect::from_min_size(
        egui::pos2(rect.right() - size_w, rect.bottom() - 2.5),
        egui::vec2(size_w, 2.5),
    );
    ui.painter().rect_filled(size_rect, 0.0, size_color);

    // Whale wall indicator dot
    if is_wall {
        ui.painter().circle_filled(
            egui::pos2(rect.left() + 4.0, rect.center().y),
            2.5,
            ACCENT_GOLD,
        );
    }

    ui.painter().text(
        egui::pos2(rect.left() + 11.0, rect.center().y),
        Align2::LEFT_CENTER,
        symbol.format_price(price),
        FontId::monospace(12.5),
        text_color,
    );
    ui.painter().text(
        egui::pos2(rect.center().x + 28.0, rect.center().y),
        Align2::RIGHT_CENTER,
        symbol.format_qty(qty),
        FontId::monospace(12.5),
        if is_wall {
            ACCENT_GOLD
        } else {
            Color32::LIGHT_GRAY
        },
    );
    ui.painter().text(
        egui::pos2(rect.right() - 10.0, rect.center().y),
        Align2::RIGHT_CENTER,
        symbol.format_qty(cum_qty),
        FontId::monospace(12.0),
        Color32::GRAY,
    );

    response.clicked()
}