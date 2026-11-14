fn main() {
    xray_runtime::init_local_offset();
    let result = tokio::runtime::Runtime::new()
        .expect("Tokio runtime")
        .block_on(async {
            let root = xray_supervisor::ipc::data_arg()?;
            xray_supervisor::ipc::serve(&root).await
        });
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
