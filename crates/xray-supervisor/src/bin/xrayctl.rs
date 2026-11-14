use xray_model::{Command, Request, Result};
#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
async fn run() -> Result<()> {
    let root = xray_supervisor::ipc::data_arg()?;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut positional = vec![];
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--data-dir" {
            i += 2;
        } else {
            positional.push(args[i].clone());
            i += 1;
        }
    }
    if positional.first().map(String::as_str) == Some("maintenance") {
        let mut options = xray_supervisor::maintenance::Options::default();
        let mut index = 1;
        while index < positional.len() {
            match positional[index].as_str() {
                "--apply" => options.apply = true,
                "--compress-operations" => options.compress_operations = true,
                "--retain-days" => {
                    index += 1;
                    options.retain_days = positional
                        .get(index)
                        .and_then(|v| v.parse().ok())
                        .ok_or_else(|| {
                            xray_model::Error::new(
                                "INVALID_ARGUMENT",
                                "--retain-days requires a positive integer",
                            )
                        })?;
                }
                _ => {
                    return Err(xray_model::Error::new(
                        "INVALID_ARGUMENT",
                        "maintenance [--apply] [--retain-days DAYS] [--compress-operations]",
                    ));
                }
            }
            index += 1;
        }
        let report = xray_supervisor::maintenance::run(&root, &options)?;
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }
    let command = match positional.first().map(String::as_str).unwrap_or("status") {
        "status" => Command::GetStatus,
        "profiles" => Command::ListProfileSummaries {
            after: positional.get(1).cloned(),
            limit: 100,
        },
        "logs" => Command::GetLogs,
        "stop" => Command::StopCore,
        "start" => Command::RestartCore,
        "resolve-proxy-conflict" => Command::ResolveProxyConflict,
        "shutdown" => Command::Shutdown,
        "operation" | "operation-details" => Command::GetOperation {
            request_id: positional.get(1).cloned().unwrap_or_default(),
        },
        "request" => {
            use std::io::Read;
            let mut text = String::new();
            std::io::stdin().read_to_string(&mut text)?;
            let request: Request = serde_json::from_str(&text)?;
            let response = xray_supervisor::ipc::call(&root, &request).await?;
            println!("{}", serde_json::to_string_pretty(&response)?);
            response.result?;
            return Ok(());
        }
        _ => {
            return Err(xray_model::Error::new(
                "INVALID_ARGUMENT",
                "Commands: maintenance [--apply] [--retain-days DAYS] [--compress-operations], status, profiles [AFTER], logs, start, stop, resolve-proxy-conflict, shutdown, operation ID, operation-details ID, request (JSON from stdin)",
            ));
        }
    };
    let status = xray_supervisor::ipc::call(&root, &Request::new(Command::GetStatus, None))
        .await?
        .result?;
    let generation = status
        .pointer("/generation/id")
        .and_then(|v| v.as_str())
        .map(str::to_owned);
    let details_id = if positional.first().map(String::as_str) == Some("operation-details") {
        positional.get(1).cloned()
    } else {
        None
    };
    let request = Request::new(command, generation);
    eprintln!("requestId: {}", request.request_id);
    let response = xray_supervisor::ipc::call(&root, &request).await?;
    let value = response.result.clone()?;
    if let Some(request_id) = details_id
        && let Some(expected_hash) = value.pointer("/details/sha256").and_then(|v| v.as_str())
    {
        let mut bytes = vec![];
        loop {
            let chunk = xray_supervisor::ipc::call(
                &root,
                &Request::new(
                    Command::GetOperationDetails {
                        request_id: request_id.clone(),
                        expected_hash: expected_hash.into(),
                        offset: bytes.len(),
                        limit: 1048576,
                    },
                    None,
                ),
            )
            .await?
            .result?;
            let data: Vec<u8> = serde_json::from_value(chunk["bytes"].clone())?;
            bytes.extend(data);
            if chunk["next_offset"].is_null() {
                break;
            }
        }
        if xray_model::hash(&bytes) != expected_hash {
            return Err(xray_model::Error::new(
                "OPERATION_DETAILS_CHANGED",
                "Detail hash differs",
            ));
        }
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::from_slice::<serde_json::Value>(&bytes)?)?
        );
    } else {
        println!("{}", serde_json::to_string_pretty(&response)?);
    }
    Ok(())
}
