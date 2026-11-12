use crate::workspace::{NumberInput, NumberInputs};
use eframe::egui;
use serde_json::{Value, json};

pub fn edit_focused(
    ui: &mut egui::Ui,
    value: &mut Value,
    section: &str,
    focus: Option<&str>,
    inputs: &mut NumberInputs,
) -> bool {
    let mut working = value.clone();
    let changed = edit_document(ui, &mut working, section, focus, inputs);
    if changed {
        *value = working;
    }
    changed
}
fn edit_document(
    ui: &mut egui::Ui,
    value: &mut Value,
    section: &str,
    focus: Option<&str>,
    inputs: &mut NumberInputs,
) -> bool {
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
                    if ui
                        .add_enabled(inputs.is_empty(), egui::Button::new("↑"))
                        .clicked()
                        && index > 0
                    {
                        action = Some((index, 0));
                    }
                    if ui
                        .add_enabled(inputs.is_empty(), egui::Button::new("↓"))
                        .clicked()
                    {
                        action = Some((index, 1));
                    }
                    if ui
                        .add_enabled(inputs.is_empty(), egui::Button::new("删除条目"))
                        .clicked()
                    {
                        action = Some((index, 2));
                    }
                });
                let path = if section == "rules" {
                    format!("/routing/rules/{index}")
                } else {
                    format!("/{section}/{index}")
                };
                egui::CollapsingHeader::new("原生字段")
                    .id_salt(index)
                    .open(focus.filter(|f| f.starts_with(&path)).map(|_| true))
                    .show(ui, |ui| {
                        changed |= node(ui, entry, 0, &path, focus, inputs);
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
fn node(
    ui: &mut egui::Ui,
    v: &mut Value,
    depth: usize,
    path: &str,
    focus: Option<&str>,
    inputs: &mut NumberInputs,
) -> bool {
    if focus == Some(path) {
        ui.scroll_to_cursor(Some(egui::Align::Center));
    }
    if depth > 12 {
        ui.label("深层结构请使用原始 JSON 编辑");
        return false;
    }
    let mut changed = false;
    match v {
        Value::Object(map) => {
            let mut remove = None;
            for (key, value) in map.iter_mut() {
                let child_path = format!("{path}/{}", key.replace('~', "~0").replace('/', "~1"));
                ui.push_id(key, |ui| {
                    ui.horizontal_top(|ui| {
                        if ui
                            .add_enabled(inputs.is_empty(), egui::Button::new("−").small())
                            .on_hover_text("删除此字段")
                            .clicked()
                        {
                            remove = Some(key.clone());
                        }
                        ui.vertical(|ui| {
                            if value.is_object() || value.is_array() {
                                egui::CollapsingHeader::new(key)
                                    .open(
                                        focus.filter(|f| f.starts_with(&child_path)).map(|_| true),
                                    )
                                    .show(ui, |ui| {
                                        changed |=
                                            node(ui, value, depth + 1, &child_path, focus, inputs);
                                    });
                            } else {
                                ui.label(key);
                                changed |= node(ui, value, depth + 1, &child_path, focus, inputs);
                            }
                        });
                    });
                });
            }
            if let Some(key) = remove {
                map.remove(&key);
                changed = true;
            }
            egui::CollapsingHeader::new("添加字段")
                .id_salt("add-field")
                .show(ui, |ui| {
                    let key_id = ui.make_persistent_id("new-key");
                    let type_id = ui.make_persistent_id("new-type");
                    let mut key = ui
                        .data_mut(|data| data.get_temp::<String>(key_id))
                        .unwrap_or_default();
                    let mut kind = ui
                        .data_mut(|data| data.get_temp::<String>(type_id))
                        .unwrap_or_else(|| "字符串".into());
                    ui.add(
                        crate::widgets::singleline(&mut key, f32::INFINITY)
                            .hint_text("字段原生名称"),
                    );
                    egui::ComboBox::from_id_salt("new-type-picker")
                        .selected_text(&kind)
                        .show_ui(ui, |ui| {
                            for label in ["字符串", "数值", "布尔", "对象", "数组", "null"]
                            {
                                ui.selectable_value(&mut kind, label.into(), label);
                            }
                        });
                    let key_text = key.trim();
                    if ui
                        .add_enabled(
                            !key_text.is_empty() && !map.contains_key(key_text),
                            egui::Button::new("加入字段"),
                        )
                        .clicked()
                    {
                        map.insert(
                            key_text.into(),
                            match kind.as_str() {
                                "数值" => json!(0),
                                "布尔" => json!(false),
                                "对象" => json!({}),
                                "数组" => json!([]),
                                "null" => Value::Null,
                                _ => json!(""),
                            },
                        );
                        key.clear();
                        changed = true;
                    }
                    ui.data_mut(|data| {
                        data.insert_temp(key_id, key);
                        data.insert_temp(type_id, kind);
                    });
                });
        }
        Value::Array(a) => {
            let mut remove = None;
            for (i, value) in a.iter_mut().enumerate() {
                ui.push_id(i, |ui| {
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(inputs.is_empty(), egui::Button::new("−"))
                            .clicked()
                        {
                            remove = Some(i);
                        }
                        changed |=
                            node(ui, value, depth + 1, &format!("{path}/{i}"), focus, inputs);
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
            let source = n.to_string();
            let mut text = inputs
                .get(path)
                .filter(|input| input.source == source)
                .map(|input| input.text.clone())
                .unwrap_or_else(|| source.clone());
            if ui
                .add(crate::widgets::singleline(&mut text, f32::INFINITY))
                .changed()
            {
                if let Ok(number) = text.parse::<serde_json::Number>() {
                    *n = number;
                    inputs.remove(path);
                } else {
                    inputs.insert(
                        path.into(),
                        NumberInput {
                            source,
                            text: text.clone(),
                        },
                    );
                }
                changed = true;
            }
            if inputs.contains_key(path) {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(
                            "数值尚未完整，输入随草稿保存；补全后才写入 JSON 并允许检查应用。",
                        )
                        .color(ui.visuals().warn_fg_color),
                    )
                    .wrap(),
                );
            }
        }
        Value::Null => {
            ui.label("null（可在原始 JSON 中编辑）");
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame(ctx: &egui::Context, value: &mut Value, events: Vec<egui::Event>) -> egui::FullOutput {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(640.0, 480.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let id = egui::Id::new("test-number-inputs");
                    let mut inputs = ctx
                        .data_mut(|d| d.get_temp::<NumberInputs>(id))
                        .unwrap_or_default();
                    node(ui, value, 0, "/number", None, &mut inputs);
                    ctx.data_mut(|d| d.insert_temp(id, inputs));
                });
            },
        )
    }
    fn replace_events(text: &str) -> Vec<egui::Event> {
        vec![
            egui::Event::Key {
                key: egui::Key::A,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers {
                    ctrl: true,
                    command: true,
                    ..Default::default()
                },
            },
            egui::Event::Text(text.into()),
        ]
    }
    #[test]
    fn incomplete_native_numbers_keep_their_buffer_until_valid_or_externally_replaced() {
        let ctx = egui::Context::default();
        let mut value = json!(0);
        let output = frame(&ctx, &mut value, vec![]);
        let pos = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text() == "0" => {
                    Some(text.pos + text.galley.size() * 0.5)
                }
                _ => None,
            })
            .unwrap();
        frame(
            &ctx,
            &mut value,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        frame(
            &ctx,
            &mut value,
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        frame(&ctx, &mut value, replace_events("-"));
        assert_eq!(value, json!(0));
        frame(&ctx, &mut value, vec![]);
        frame(&ctx, &mut value, vec![egui::Event::Text("1".into())]);
        assert_eq!(value, json!(-1));
        frame(&ctx, &mut value, replace_events("1."));
        assert_eq!(value, json!(-1));
        frame(&ctx, &mut value, vec![egui::Event::Text("5".into())]);
        assert_eq!(value, json!(1.5));
        value = json!(99);
        frame(&ctx, &mut value, vec![]);
        frame(&ctx, &mut value, vec![egui::Event::Text("0".into())]);
        assert_eq!(value, json!(990));
    }
    #[test]
    fn viewing_absent_or_malformed_sections_never_materializes_or_replaces_them() {
        for section in ["outbounds", "inbounds", "rules"] {
            for value in [
                json!({}),
                json!({"routing":null,"outbounds":false,"inbounds":"keep"}),
            ] {
                let ctx = egui::Context::default();
                let mut edited = value.clone();
                let _ = ctx.run(Default::default(), |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        assert!(!edit_focused(
                            ui,
                            &mut edited,
                            section,
                            None,
                            &mut NumberInputs::new()
                        ));
                    });
                });
                assert_eq!(edited, value);
            }
        }
    }
}
