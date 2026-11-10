use eframe::egui;
use serde_json::{Value, json};

pub fn edit(ui: &mut egui::Ui, value: &mut Value, section: &str) -> bool {
    let Some(object) = value.as_object_mut() else {
        return false;
    };
    let mut changed = false;
    let array = if section == "rules" {
        object
            .entry("routing")
            .or_insert(json!({}))
            .as_object_mut()
            .map(|m| m.entry("rules").or_insert(json!([])))
    } else {
        Some(object.entry(section).or_insert(json!([])))
    };
    if let Some(array) = array.and_then(Value::as_array_mut) {
        let mut action = None;
        for (index, entry) in array.iter_mut().enumerate() {
            let label = entry
                .get("tag")
                .and_then(Value::as_str)
                .or_else(|| entry.get("outboundTag").and_then(Value::as_str))
                .unwrap_or("原生条目")
                .to_string();
            ui.push_id(index, |ui| {
                ui.add(egui::Label::new(format!("{} · {label}", index + 1)).wrap());
                ui.horizontal(|ui| {
                    if ui.button("↑").clicked() && index > 0 {
                        action = Some((index, 0));
                    }
                    if ui.button("↓").clicked() {
                        action = Some((index, 1));
                    }
                    if ui.button("删除条目").clicked() {
                        action = Some((index, 2));
                    }
                });
                egui::CollapsingHeader::new("原生字段")
                    .id_salt(index)
                    .show(ui, |ui| {
                        changed |= node(ui, entry, 0);
                    });
            });
        }
        if let Some((index, kind)) = action {
            match kind {
                0 => array.swap(index, index - 1),
                1 if index + 1 < array.len() => array.swap(index, index + 1),
                2 => {
                    array.remove(index);
                }
                _ => {}
            }
            changed = true;
        }
        if ui.button("添加条目").clicked() {
            array.push(match section {
                "inbounds" => json!({
                    "tag": format!("in-{}", array.len() + 1),
                    "listen": "127.0.0.1",
                    "port": 10810 + array.len(),
                    "protocol": "socks",
                    "settings": {"udp": true}
                }),
                "outbounds" => json!({
                    "tag": format!("out-{}", array.len() + 1),
                    "protocol": "freedom",
                    "settings": {}
                }),
                _ => json!({
                    "type": "field",
                    "domain": ["full:example.com"],
                    "outboundTag": "direct"
                }),
            });
            changed = true;
        }
    } else {
        ui.label("此字段不是原生数组，请在原始 JSON 中修正。");
    }
    changed
}
fn node(ui: &mut egui::Ui, v: &mut Value, depth: usize) -> bool {
    if depth > 12 {
        ui.label("深层结构请使用原始 JSON 编辑");
        return false;
    }
    let mut changed = false;
    match v {
        Value::Object(map) => {
            for (key, value) in map {
                ui.push_id(key, |ui| {
                    if value.is_object() || value.is_array() {
                        egui::CollapsingHeader::new(key).show(ui, |ui| {
                            changed |= node(ui, value, depth + 1);
                        });
                    } else {
                        ui.horizontal(|ui| {
                            let width = 140.0_f32.min(ui.available_width() * 0.35);
                            ui.add_sized(
                                [width, crate::widgets::CONTROL_HEIGHT],
                                egui::Label::new(key).wrap(),
                            );
                            changed |= node(ui, value, depth + 1);
                        });
                    }
                });
            }
        }
        Value::Array(a) => {
            let mut remove = None;
            for (i, value) in a.iter_mut().enumerate() {
                ui.push_id(i, |ui| {
                    ui.horizontal(|ui| {
                        if ui.button("−").clicked() {
                            remove = Some(i);
                        }
                        changed |= node(ui, value, depth + 1);
                    });
                });
            }
            if let Some(i) = remove {
                a.remove(i);
                changed = true;
            }
            if ui.button("添加值").clicked() {
                a.push(json!(""));
                changed = true;
            }
        }
        Value::String(s) => {
            changed |= ui
                .add(crate::widgets::singleline(s, f32::INFINITY))
                .changed()
        }
        Value::Bool(b) => changed |= ui.checkbox(b, "").changed(),
        Value::Number(n) => {
            let mut text = n.to_string();
            if ui
                .add(crate::widgets::singleline(&mut text, f32::INFINITY))
                .changed()
                && let Ok(number) = text.parse::<serde_json::Number>()
            {
                *n = number;
                changed = true;
            }
        }
        Value::Null => {
            ui.label("null（可在原始 JSON 中编辑）");
        }
    }
    changed
}
