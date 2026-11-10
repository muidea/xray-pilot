fn main() {
    println!("cargo:rerun-if-changed=assets/xray-pilot.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("assets/xray-pilot.ico")
            .set("ProductName", "Xray Pilot")
            .set("FileDescription", "Xray Pilot")
            .compile()
            .expect("Embed Windows application icon");
    }
}
