use tray_icon::{
    Icon, TrayIcon, TrayIconBuilder,
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
};
pub struct Tray {
    _icon: TrayIcon,
    actions: std::sync::mpsc::Receiver<String>,
    items: Vec<(MenuItem, &'static str)>,
}
pub struct State {
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
        for (title, key) in [
            ("打开 Xray Pilot", "open"),
            ("启动 / 重启 Core", "start"),
            ("停止 Core", "stop"),
            ("不接管系统代理（恢复先前设置）", "direct"),
            ("使用本机 HTTP 代理", "global"),
            ("仅退出界面，后台继续运行", "exit-gui"),
            ("退出程序（停止 Core 并恢复代理）", "quit"),
        ] {
            if matches!(key, "direct" | "exit-gui") {
                menu.append(&PredefinedMenuItem::separator())?;
            }
            let item = MenuItem::new(title, true, None);
            menu.append(&item)?;
            items.push((item, key));
        }
        let mut pixels = vec![0; 32 * 32 * 4];
        for y in 0usize..32 {
            for x in 0usize..32 {
                let i = (y * 32 + x) * 4;
                let cross = x.abs_diff(y) < 4 || x.abs_diff(31 - y) < 4;
                pixels[i] = if cross { 240 } else { 35 };
                pixels[i + 1] = if cross { 245 } else { 90 };
                pixels[i + 2] = if cross { 255 } else { 155 };
                pixels[i + 3] = 255;
            }
        }
        let icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("Xray Pilot")
            .with_icon(Icon::from_rgba(pixels, 32, 32)?)
            .build()?;
        let (tx, actions) = std::sync::mpsc::channel();
        let ids: Vec<_> = items
            .iter()
            .map(|(item, key)| (item.id().clone(), *key))
            .collect();
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            if let Some((_, key)) = ids.iter().find(|(id, _)| id == &event.id) {
                let _ = tx.send((*key).into());
                wakeup.request();
            }
        }));
        Ok(Self {
            _icon: icon,
            actions,
            items,
        })
    }
    pub fn actions(&self) -> Vec<String> {
        self.actions.try_iter().collect()
    }
    pub fn update(&self, state: State) {
        for (item, key) in &self.items {
            item.set_enabled(match *key {
                "start" => state.can_write,
                "stop" => state.can_stop,
                "direct" | "global" => state.can_write && state.running,
                "exit-gui" => state.can_exit_gui,
                "quit" => !state.quitting,
                _ => true,
            });
        }
    }
}
