use eframe::egui;

pub const CONTROL_HEIGHT: f32 = 36.0;

/// A single-line field uses the same outer height and text alignment as buttons.
/// `width` includes the horizontal padding; infinity fills the available cell.
pub fn singleline(value: &mut String, width: f32) -> egui::TextEdit<'_> {
    egui::TextEdit::singleline(value)
        .desired_width((width - 20.0).max(24.0))
        .min_size(egui::vec2(0.0, CONTROL_HEIGHT))
        .margin(egui::Margin::symmetric(10, 8))
        .vertical_align(egui::Align::Center)
}
