use crate::book::Metrics;
use crate::symbol::Symbol;
use crate::trades::FlowStats;
use crate::ui::{format_notional, ACCENT_BLUE, ACCENT_GOLD, ASK_TEXT, BID_TEXT};
use egui::{Color32, RichText, Ui};

pub struct HeaderAction {
    pub new_symbol: Option<Symbol>,
    pub new_bucket: Option<i64>,
    pub toggle_pause: bool,
    pub clear_tape: bool,
}

pub fn render_header(
    ui: &mut Ui,
    symbol: Symbol,
    bucket_units: i64,
    paused: bool,
    metrics: &Metrics,
    flow: &FlowStats,
    cvd: f64,
) -> HeaderAction {
    let mut action = HeaderAction {
        new_symbol: None,
        new_bucket: None,
        toggle_pause: false,
        clear_tape: false,
    };

    ui.horizontal_wrapped(|ui| {
        ui.heading(
            RichText::new("⚡ WASM L2 MICROSTRUCTURE TERMINAL")
                .color(ACCENT_BLUE)
                .strong(),
        );
        ui.separator();

        for sym in Symbol::ALL {
            if ui.selectable_label(symbol == sym, sym.label()).clicked() {
                action.new_symbol = Some(sym);
            }
        }

        ui.separator();
        ui.label("Tick:");
        for &(units, label) in symbol.tick_presets() {
            if ui.selectable_label(bucket_units == units, label).clicked() {
                action.new_bucket = Some(units);
            }
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let (btn_text, btn_color) = if paused {
                ("▶ RESUME", BID_TEXT)
            } else {
                ("⏸ PAUSE", ASK_TEXT)
            };
            if ui
                .button(RichText::new(btn_text).color(btn_color).strong())
                .clicked()
            {
                action.toggle_pause = true;
            }
            if ui.button("↺ Reset CVD").clicked() {
                action.clear_tape = true;
            }
        });
    });

    ui.add_space(4.0);
    ui.separator();
    ui.add_space(4.0);

    // Quantitative Microstructure KPI Strip
    ui.horizontal_wrapped(|ui| {
        render_kpi(
            ui,
            "MID PRICE",
            &format!("${}", symbol.format_price(metrics.mid)),
            Color32::WHITE,
        );
        ui.separator();

        let mp_color = if metrics.micro_delta_bps >= 0.0 {
            BID_TEXT
        } else {
            ASK_TEXT
        };
        render_kpi(
            ui,
            "MICROPRICE (VOL-WEIGHTED)",
            &format!(
                "${} ({:+.2} bps)",
                symbol.format_price(metrics.microprice),
                metrics.micro_delta_bps
            ),
            mp_color,
        );
        ui.separator();

        render_kpi(
            ui,
            "SPREAD",
            &format!(
                "${} ({:.2} bps)",
                symbol.format_price(metrics.spread),
                metrics.spread_bps
            ),
            ACCENT_GOLD,
        );
        ui.separator();

        let obi_pct = metrics.obi_signed_pct();
        let obi_color = if obi_pct >= 0.0 { BID_TEXT } else { ASK_TEXT };
        render_kpi(
            ui,
            "L10 IMBALANCE",
            &format!(
                "{:+.1}% ({} / {})",
                obi_pct,
                format_notional(metrics.bid_notional),
                format_notional(metrics.ask_notional)
            ),
            obi_color,
        );
        ui.separator();

        let cvd_color = if cvd >= 0.0 { BID_TEXT } else { ASK_TEXT };
        render_kpi(
            ui,
            "SESSION CVD",
            &format!("{:+.2} {}", cvd, symbol.base_asset()),
            cvd_color,
        );
        ui.separator();

        render_kpi(
            ui,
            "60S FLOW / VWAP",
            &format!(
                "{:.1} tr/s | ${}",
                flow.trades_per_sec,
                if flow.vwap > 0.0 {
                    symbol.format_price(flow.vwap)
                } else {
                    "--".into()
                }
            ),
            Color32::LIGHT_GRAY,
        );
    });

    action
}

fn render_kpi(ui: &mut Ui, title: &str, value: &str, color: Color32) {
    ui.vertical(|ui| {
        ui.label(RichText::new(title).size(10.0).color(Color32::GRAY));
        ui.monospace(RichText::new(value).size(13.5).color(color).strong());
    });
}