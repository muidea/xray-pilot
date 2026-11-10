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
        let image = crate::icon::tray();
        let icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("Xray Pilot")
            .with_icon(Icon::from_rgba(image.rgba, image.width, image.height)?)
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
