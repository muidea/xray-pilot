#![cfg_attr(windows, windows_subsystem = "windows")]
mod app;
mod dependencies;
mod guided_config;
mod icon;
mod native_editor;
mod path_display;
mod preferences;
#[cfg(windows)]
mod tray;
mod wakeup;
mod widgets;
mod workspace;

fn main() -> eframe::Result {
    let root = xray_supervisor::ipc::data_arg().expect("Valid data directory");
    std::fs::create_dir_all(&root).expect("Writable data directory");
    let root = std::fs::canonicalize(root).expect("Data directory");
    if std::env::args_os().any(|a| a == "--supervisor") {
        let runtime = tokio::runtime::Runtime::new().expect("Runtime");
        if let Err(e) = runtime.block_on(xray_supervisor::ipc::serve(&root)) {
            eprintln!("{e}");
            std::process::exit(1);
        }
        return Ok(());
    }
    let background = cfg!(windows) && std::env::args_os().any(|a| a == "--background");
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 760.0])
            .with_min_inner_size([760.0, 500.0])
            .with_icon(std::sync::Arc::new(icon::application()))
            .with_visible(!background),
        ..Default::default()
    };
    eframe::run_native(
        "Xray Pilot",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, root)))),
    )
}
