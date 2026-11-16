pub mod chart;
pub mod header;
pub mod ladder;
pub mod simulator;
pub mod tape;

use egui::Color32;

pub const BG_PRIMARY: Color32 = Color32::from_rgb(13, 17, 23);
pub const BG_SURFACE: Color32 = Color32::from_rgb(9, 12, 17);
pub const BORDER_SUBTLE: Color32 = Color32::from_rgb(35, 42, 54);

pub const BID_TEXT: Color32 = Color32::from_rgb(46, 160, 67);
pub const BID_FILL_LIGHT: Color32 = Color32::from_rgba_unmultiplied(39, 174, 96, 36);
pub const BID_FILL_STRONG: Color32 = Color32::from_rgba_unmultiplied(39, 174, 96, 95);

pub const ASK_TEXT: Color32 = Color32::from_rgb(248, 81, 73);
pub const ASK_FILL_LIGHT: Color32 = Color32::from_rgba_unmultiplied(235, 87, 87, 36);
pub const ASK_FILL_STRONG: Color32 = Color32::from_rgba_unmultiplied(235, 87, 87, 95);

pub const ACCENT_BLUE: Color32 = Color32::from_rgb(88, 166, 255);
pub const ACCENT_GOLD: Color32 = Color32::from_rgb(242, 201, 76);

pub fn format_notional(usd: f64) -> String {
    if usd >= 1_000_000.0 {
        format!("${:.2}M", usd / 1_000_000.0)
    } else if usd >= 1_000.0 {
        format!("${:.1}K", usd / 1_000.0)
    } else {
        format!("${:.0}", usd)
    }
}