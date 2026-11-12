use tray_icon::{
    Icon, TrayIcon, TrayIconBuilder,
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu},
};
type Bindings =
    std::sync::Arc<std::sync::Mutex<std::collections::HashMap<tray_icon::menu::MenuId, String>>>;
pub struct Tray {
    _icon: TrayIcon,
    actions: std::sync::mpsc::Receiver<String>,
    items: Vec<(MenuItem, &'static str)>,
    status: MenuItem,
    profiles: Submenu,
    profile_items: Vec<MenuItem>,
    profile_snapshot: Vec<(String, String)>,
    bindings: Bindings,
}
pub struct State {
    pub status: String,
    pub can_write: bool,
    pub can_stop: bool,
    pub can_exit_gui: bool,
    pub running: bool,
    pub quitting: bool,
}
impl Tray {
    pub fn new(wakeup: crate::wakeup::Wakeup) -> Result<Self, Box<dyn std::error::Error>> {
        let menu = Menu::new();
        let mut items = vec![];
        let status = MenuItem::new("状态：未读取", false, None);
        menu.append(&status)?;
        menu.append(&PredefinedMenuItem::separator())?;
        let profiles = Submenu::new("配置（选择后需检查并确认应用）", true);
        for (title, key) in [
            ("打开 Xray Pilot", "open"),
            ("启动 / 重启 Core", "start"),
            ("停止 Core", "stop"),
            ("不接管系统代理（恢复先前设置）", "direct"),
            ("使用本机 HTTP 代理", "global"),
            ("使用 PAC 自动代理", "pac"),
            ("退出…", "exit"),
        ] {
            if matches!(key, "direct" | "exit") {
                menu.append(&PredefinedMenuItem::separator())?;
            }
            let item = MenuItem::new(title, true, None);
            menu.append(&item)?;
            items.push((item, key));
            if key == "open" {
                menu.append(&profiles)?;
            }
        }
        let image = crate::icon::tray();
        let icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("Xray Pilot")
            .with_icon(Icon::from_rgba(image.rgba, image.width, image.height)?)
            .build()?;
        let (tx, actions) = std::sync::mpsc::channel();
        let bindings = std::sync::Arc::new(std::sync::Mutex::new(
            items
                .iter()
                .map(|(item, key)| (item.id().clone(), (*key).to_string()))
                .collect::<std::collections::HashMap<_, _>>(),
        ));
        let events = bindings.clone();
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            let action = events
                .lock()
                .ok()
                .and_then(|ids| ids.get(&event.id).cloned());
            if let Some(action) = action {
                let _ = tx.send(action);
                wakeup.request();
            }
        }));
        Ok(Self {
            _icon: icon,
            actions,
            items,
            status,
            profiles,
            profile_items: vec![],
            profile_snapshot: vec![],
            bindings,
        })
    }
    pub fn actions(&self) -> Vec<String> {
        self.actions.try_iter().collect()
    }
    pub fn update(&mut self, state: State, profiles: Vec<(String, String)>) {
        self.status.set_text(format!("状态：{}", state.status));
        if self.profile_snapshot != profiles {
            for item in self.profile_items.drain(..) {
                let _ = self.profiles.remove(&item);
                if let Ok(mut ids) = self.bindings.lock() {
                    ids.remove(item.id());
                }
            }
            for (id, title) in &profiles {
                let title = title.replace('&', "&&").replace(['\t', '\r', '\n'], " ");
                let item = MenuItem::new(title, true, None);
                if self.profiles.append(&item).is_ok() {
                    if let Ok(mut ids) = self.bindings.lock() {
                        ids.insert(item.id().clone(), format!("profile:{id}"));
                    }
                    self.profile_items.push(item);
                }
            }
            if self.profile_items.len() == profiles.len() {
                self.profile_snapshot = profiles;
            } else {
                self.profile_snapshot.clear();
            }
        }
        self.profiles
            .set_enabled(!self.profile_items.is_empty() && state.can_exit_gui);
        for (item, key) in &self.items {
            item.set_enabled(match *key {
                "start" => state.can_write,
                "stop" => state.can_stop,
                "direct" | "global" | "pac" => state.can_write && state.running,
                "exit" => !state.quitting,
                _ => true,
            });
        }
    }
}
