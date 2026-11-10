use tray_icon::{
    Icon, TrayIcon, TrayIconBuilder,
    menu::{Menu, MenuEvent, MenuItem},
};
pub struct Tray {
    _icon: TrayIcon,
    items: Vec<(MenuItem, &'static str)>,
}
impl Tray {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
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
        Ok(Self { _icon: icon, items })
    }
    pub fn actions(&self) -> Vec<String> {
        let mut actions = vec![];
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            if let Some((_, key)) = self.items.iter().find(|(item, _)| item.id() == &event.id) {
                actions.push((*key).into());
            }
        }
        actions
    }
}
