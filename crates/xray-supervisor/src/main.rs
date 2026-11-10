#[tokio::main]
async fn main() {
    let result = async {
        let root = xray_supervisor::ipc::data_arg()?;
        xray_supervisor::ipc::serve(&root).await
    }
    .await;
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
