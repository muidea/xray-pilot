use super::*;

pub(super) fn json_editor(
    ui: &mut egui::Ui,
    json: &mut String,
    id: &str,
    height: f32,
) -> egui::Response {
    json_area(ui, json, id, height, true)
}

pub(super) fn json_viewer(
    ui: &mut egui::Ui,
    json: &mut String,
    id: &str,
    height: f32,
) -> egui::Response {
    json_area(ui, json, id, height, false)
}

fn json_area(
    ui: &mut egui::Ui,
    json: &mut String,
    id: &str,
    height: f32,
    interactive: bool,
) -> egui::Response {
    let font = egui::FontId::monospace(14.0);
    let color = ui.visuals().text_color();
    let line_count = json.bytes().filter(|b| *b == b'\n').count() + 1;
    let numbers = (1..=line_count)
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    let mut layouter = |ui: &egui::Ui, text: &dyn egui::TextBuffer, _width: f32| {
        ui.fonts_mut(|fonts| fonts.layout_no_wrap(text.as_str().into(), font.clone(), color))
    };
    // The frame belongs to the viewport, not the (potentially very long)
    // document. Short documents therefore keep the full editor background too.
    egui::Frame::new()
        .fill(if ui.visuals().dark_mode {
            Color32::from_rgb(13, 18, 25)
        } else {
            Color32::from_rgb(245, 247, 250)
        })
        .stroke(egui::Stroke::new(
            1.0,
            ui.visuals().widgets.noninteractive.bg_stroke.color,
        ))
        .corner_radius(6)
        .show(ui, |ui| {
            let width = ui.available_width();
            let height = (height - 2.0).max(1.0);
            egui::ScrollArea::both()
                .id_salt(id)
                .max_width(width)
                .max_height(height)
                .min_scrolled_width(0.0)
                .min_scrolled_height(height)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.horizontal_top(|ui| {
                        let gutter = egui::Frame::new()
                            .inner_margin(egui::Margin::symmetric(6, 6))
                            .show(ui, |ui| {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(numbers).font(font.clone()).weak(),
                                    )
                                    .extend(),
                                );
                            });
                        ui.add(
                            egui::TextEdit::multiline(json)
                                .id_salt(id)
                                .code_editor()
                                .interactive(interactive)
                                .font(font.clone())
                                .layouter(&mut layouter)
                                .frame(false)
                                .margin(egui::Margin::symmetric(8, 6))
                                .desired_width(
                                    (width - gutter.response.rect.width() - 30.0).max(40.0),
                                )
                                .desired_rows(1)
                                .min_size(egui::vec2(0.0, (height - 12.0).max(0.0))),
                        )
                    })
                    .inner
                })
                .inner
        })
        .inner
}

/// Returns whether fields need attention. Import of a valid object remains allowed.
pub(super) fn config_review(ui: &mut egui::Ui, value: &serde_json::Value) -> bool {
    let issues = xray_config::inspect_config(value);
    let blocking = issues.iter().filter(|i| i.blocking).count();
    if issues.is_empty() {
        ui.label(
            RichText::new("已完成基础字段核对；保存后仍需使用所选 Core 检查并预览。")
                .small()
                .weak(),
        );
        return false;
    }
    ui.label(
        RichText::new(if blocking > 0 {
            format!("可先保存 · {blocking} 项需要补充或修正后再检查运行")
        } else {
            "可先保存 · 有配置用途需要核对".into()
        })
        .color(ui.visuals().warn_fg_color),
    );
    let count = issues.len();
    egui::CollapsingHeader::new(format!("查看 {count} 项补充 / 核对提示"))
        .id_salt("config-review")
        .default_open(blocking > 0)
        .show(ui, |ui| {
            for issue in issues.into_iter().take(20) {
                ui.label(RichText::new(&issue.path).monospace().strong());
                ui.add(egui::Label::new(issue.message).wrap());
                ui.add(egui::Label::new(RichText::new(issue.help).weak()).wrap());
                ui.add_space(6.0);
            }
            if count > 20 {
                ui.label(format!(
                    "另有 {} 项，请先处理上方字段后重新核对。",
                    count - 20
                ));
            }
        });
    true
}
