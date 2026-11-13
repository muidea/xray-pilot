use eframe::egui;

/// Wake the UI even when its Windows viewport is hidden in the tray.
#[derive(Clone)]
pub struct Wakeup {
    context: egui::Context,
    #[cfg(windows)]
    window: std::sync::Arc<std::sync::atomic::AtomicIsize>,
}

impl Wakeup {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        Self {
            context: cc.egui_ctx.clone(),
            #[cfg(windows)]
            window: {
                use raw_window_handle::{HasWindowHandle, RawWindowHandle};
                let handle = cc.window_handle().ok().and_then(|h| match h.as_raw() {
                    RawWindowHandle::Win32(h) => Some(h.hwnd.get()),
                    _ => None,
                });
                std::sync::Arc::new(std::sync::atomic::AtomicIsize::new(handle.unwrap_or(0)))
            },
        }
    }

    pub fn request(&self) {
        self.context.request_repaint();
        #[cfg(windows)]
        {
            use std::sync::atomic::Ordering;
            use windows_sys::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_PAINT};
            let window = self.window.load(Ordering::Acquire);
            if window != 0 {
                // winit's RedrawWindow does not deliver paints to a hidden HWND.
                // Post the paint asynchronously so it runs outside the tray menu callback.
                unsafe { PostMessageW(window as _, WM_PAINT, 0, 0) };
            }
        }
    }

    pub fn set_cloaked(&self, cloaked: bool) {
        #[cfg(windows)]
        {
            use std::sync::atomic::Ordering;
            use windows_sys::Win32::Graphics::Dwm::{DWMWA_CLOAK, DwmSetWindowAttribute};
            let window = self.window.load(Ordering::Acquire);
            if window != 0 {
                let value: u32 = u32::from(cloaked);
                unsafe {
                    let _ = DwmSetWindowAttribute(
                        window as _,
                        DWMWA_CLOAK as u32,
                        (&raw const value).cast(),
                        std::mem::size_of::<u32>() as u32,
                    );
                }
            }
        }
        #[cfg(not(windows))]
        let _ = cloaked;
    }

    pub fn reveal(&self) {
        self.set_cloaked(false);
        self.context
            .send_viewport_cmd(egui::ViewportCommand::Visible(true));
        self.context.send_viewport_cmd(egui::ViewportCommand::Focus);
        self.request();
    }

    pub fn close(&self) {
        #[cfg(windows)]
        self.window.store(0, std::sync::atomic::Ordering::Release);
    }
}
