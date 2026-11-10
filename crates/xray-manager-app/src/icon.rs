//! Shared source for the tray, window title and taskbar icons.
pub fn application() -> eframe::egui::IconData {
    eframe::icon_data::from_png_bytes(include_bytes!("../assets/xray-pilot.png"))
        .expect("Embedded application icon")
}
#[cfg(windows)]
pub fn tray() -> eframe::egui::IconData {
    eframe::icon_data::from_png_bytes(include_bytes!("../assets/xray-pilot-tray.png"))
        .expect("Embedded tray icon")
}

#[cfg(windows)]
pub fn configure_taskbar(cc: &eframe::CreationContext<'_>) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GWLP_HINSTANCE, GetWindowLongPtrW, ICON_BIG, IMAGE_ICON, LR_SHARED, LoadImageW,
        SendMessageW, WM_SETICON,
    };
    let Ok(handle) = cc.window_handle() else {
        return;
    };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return;
    };
    // winit sets only the small title icon through with_icon. Assign the big
    // taskbar icon from our embedded ICO too. LR_SHARED keeps ownership in Windows.
    unsafe {
        let window = handle.hwnd.get() as _;
        let module = GetWindowLongPtrW(window, GWLP_HINSTANCE) as _;
        let icon = LoadImageW(
            module,
            std::ptr::without_provenance(1),
            IMAGE_ICON,
            64,
            64,
            LR_SHARED,
        );
        if !icon.is_null() {
            SendMessageW(window, WM_SETICON, ICON_BIG as usize, icon as isize);
        }
    }
}
