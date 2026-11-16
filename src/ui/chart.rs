use crate::book::Ladder;
use crate::symbol::Symbol;
use crate::ui::{ASK_TEXT, BG_SURFACE, BID_TEXT, BORDER_SUBTLE};
use egui::{Align2, Color32, FontId, Sense, Stroke, Ui};

pub fn render_depth_chart(
    ui: &mut Ui,
    symbol: Symbol,
    ladder: &Ladder,
    mid_price: f64,
    height: f32,
) {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), Sense::hover());
    let painter = ui.painter_at(rect);

    painter.rect_filled(rect, 4.0, BG_SURFACE);
    painter.rect_stroke(rect, 4.0, Stroke::new(1.0, BORDER_SUBTLE));

    if ladder.bids.is_empty() || ladder.asks.is_empty() || mid_price <= 0.0 {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "Awaiting L2 Depth Stream...",
            FontId::monospace(14.0),
            Color32::GRAY,
        );
        return;
    }

    let mut bid_curve = Vec::with_capacity(ladder.bids.len());
    let mut b_sum = 0.0;
    for lvl in &ladder.bids {
        b_sum += lvl.qty;
        bid_curve.push((lvl.price, b_sum));
    }

    let mut ask_curve = Vec::with_capacity(ladder.asks.len());
    let mut a_sum = 0.0;
    for lvl in &ladder.asks {
        a_sum += lvl.qty;
        ask_curve.push((lvl.price, a_sum));
    }

    let min_price = bid_curve.last().map(|x| x.0).unwrap_or(mid_price * 0.999);
    let max_price = ask_curve.last().map(|x| x.0).unwrap_or(mid_price * 1.001);
    let price_span = (max_price - min_price).max(1e-6);
    let max_vol = b_sum.max(a_sum).max(1e-6);

    let plot_rect = egui::Rect::from_min_max(
        rect.min + egui::vec2(10.0, 14.0),
        rect.max - egui::vec2(10.0, 22.0),
    );

    // Subtle horizontal gridlines
    for i in 1..=3 {
        let y = plot_rect.bottom() - (i as f32 / 4.0) * plot_rect.height();
        painter.line_segment(
            [egui::pos2(plot_rect.left(), y), egui::pos2(plot_rect.right(), y)],
            Stroke::new(0.5, Color32::from_gray(32)),
        );
    }

    let price_to_x = |p: f64| -> f32 {
        let ratio = ((p - min_price) / price_span).clamp(0.0, 1.0) as f32;
        plot_rect.left() + ratio * plot_rect.width()
    };

    let vol_to_y = |v: f64| -> f32 {
        let ratio = (v / max_vol).clamp(0.0, 1.0) as f32;
        plot_rect.bottom() - ratio * plot_rect.height()
    };

    // Bid Stepped Wall
    let bid_fill = Color32::from_rgba_unmultiplied(46, 160, 67, 50);
    let bid_stroke = Stroke::new(2.0, BID_TEXT);
    let mut prev_x = price_to_x(bid_curve[0].0);
    let mut prev_y = plot_rect.bottom();

    for &(p, cum_v) in &bid_curve {
        let x = price_to_x(p);
        let y = vol_to_y(cum_v);
        let fill_rect = egui::Rect::from_min_max(
            egui::pos2(x.min(prev_x), y),
            egui::pos2(x.max(prev_x), plot_rect.bottom()),
        );
        painter.rect_filled(fill_rect, 0.0, bid_fill);
        painter.line_segment([egui::pos2(prev_x, prev_y), egui::pos2(prev_x, y)], bid_stroke);
        painter.line_segment([egui::pos2(prev_x, y), egui::pos2(x, y)], bid_stroke);
        prev_x = x;
        prev_y = y;
    }

    // Ask Stepped Wall
    let ask_fill = Color32::from_rgba_unmultiplied(248, 81, 73, 50);
    let ask_stroke = Stroke::new(2.0, ASK_TEXT);
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
        Stroke::new(1.0, Color32::from_gray(110)),
    );

    // Axis Labels
    painter.text(
        egui::pos2(plot_rect.left(), rect.bottom() - 4.0),
        Align2::LEFT_BOTTOM,
        format!("${}", symbol.format_price(min_price)),
        FontId::monospace(11.0),
        Color32::GRAY,
    );
    painter.text(
        egui::pos2(mid_x, rect.bottom() - 4.0),
        Align2::CENTER_BOTTOM,
        format!("Mid: ${}", symbol.format_price(mid_price)),
        FontId::monospace(11.0),
        Color32::LIGHT_GRAY,
    );
    painter.text(
        egui::pos2(plot_rect.right(), rect.bottom() - 4.0),
        Align2::RIGHT_BOTTOM,
        format!("${}", symbol.format_price(max_price)),
        FontId::monospace(11.0),
        Color32::GRAY,
    );

    // Interactive Hover Crosshair
    if let Some(pointer) = response.hover_pos() {
        if plot_rect.contains(pointer) {
            let ratio = ((pointer.x - plot_rect.left()) / plot_rect.width()) as f64;
            let hovered_price = min_price + ratio * price_span;
            let dist_bps = ((hovered_price - mid_price) / mid_price) * 10_000.0;

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
                Stroke::new(1.0, Color32::WHITE),
            );

            painter.text(
                egui::pos2(pointer.x, plot_rect.top() + 4.0),
                Align2::CENTER_TOP,
                format!(
                    "${} ({:+.1} bps) | Depth: {} {}",
                    symbol.format_price(hovered_price),
                    dist_bps,
                    symbol.format_qty(cum_at_hover),
                    symbol.base_asset()
                ),
                FontId::monospace(11.5),
                Color32::WHITE,
            );
        }
    }
}