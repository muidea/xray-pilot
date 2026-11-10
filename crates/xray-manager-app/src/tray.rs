use tray_icon::{
    Icon, TrayIcon, TrayIconBuilder,
    menu::{Menu, MenuEvent, MenuItem},
};
pub struct Tray {
    _icon: TrayIcon,
    actions: std::sync::mpsc::Receiver<String>,
}
impl Tray {
    pub fn new(wakeup: crate::wakeup::Wakeup) -> Result<Self, Box<dyn std::error::Error>> {
        let menu = Menu::new();
        let mut items = vec![];
        for (title, key) in [
            ("Open Xray Pilot", "open"),
            ("Start / restart Core", "start"),
            ("Stop Core", "stop"),
            ("Direct — restore previous proxy", "direct"),
            ("Global proxy", "global"),
            ("Exit GUI (keep Core)", "exit-gui"),
            ("Exit and restore proxy", "quit"),
        ] {
            let item = MenuItem::new(title, true, None);
            menu.append(&item)?;
            items.push((item.id().clone(), key));
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
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            if let Some((_, key)) = items.iter().find(|(id, _)| id == &event.id) {
                let _ = tx.send((*key).into());
                wakeup.request();
            }
        }));
        Ok(Self {
            _icon: icon,
            actions,
        })
    }
    pub fn actions(&self) -> Vec<String> {
        self.actions.try_iter().collect()
    }
}
