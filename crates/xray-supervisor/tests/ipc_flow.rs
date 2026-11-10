use std::{
    process::{Child, Command, Stdio},
    time::Duration,
};
use xray_model::{Command as Operation, Request};

struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
#[tokio::test]
async fn independent_supervisor_round_trip_and_persistent_dedup() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("data with spaces 测试");
    std::fs::create_dir_all(&root).unwrap();
    let supervisor = std::env::var_os("XRAY_TEST_SUPERVISOR")
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_xray-supervisor").into());
    let mut server = Server(
        Command::new(supervisor)
            .arg("--data-dir")
            .arg(&root)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let ping = Request::new(Operation::GetStatus, None);
    let mut connected = false;
    for _ in 0..100 {
        if xray_supervisor::ipc::call(&root, &ping).await.is_ok() {
            connected = true;
            break;
        }
        assert!(
            server.0.try_wait().unwrap().is_none(),
            "Supervisor exited during startup"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(connected, "Supervisor did not become available");
    // Repeated connections must not inherit a previous named-pipe client's buffered EOF/data.
    for _ in 0..8 {
        xray_supervisor::ipc::call(&root, &ping)
            .await
            .unwrap()
            .result
            .unwrap();
    }
    let request = Request::new(
        Operation::CreateProfile {
            profile_id: "ipc".into(),
            json: "{}".into(),
        },
        None,
    );
    let response = xray_supervisor::ipc::call(&root, &request)
        .await
        .unwrap()
        .result
        .unwrap();
    assert_eq!(
        xray_supervisor::ipc::call(&root, &request)
            .await
            .unwrap()
            .result
            .unwrap(),
        response
    );
    let shutdown = Request::new(Operation::Shutdown, None);
    xray_supervisor::ipc::call(&root, &shutdown)
        .await
        .unwrap()
        .result
        .unwrap();
    for _ in 0..100 {
        if let Some(status) = server.0.try_wait().unwrap() {
            assert!(status.success());
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("Supervisor did not shut down");
}

#[tokio::test]
async fn maximum_profiles_have_paged_summaries_and_hash_bound_operation_details() {
    use serde_json::Value;
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("large profiles");
    std::fs::create_dir_all(&root).unwrap();
    let executable = std::env::var_os("XRAY_TEST_SUPERVISOR")
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_xray-supervisor").into());
    let mut server = Server(
        Command::new(executable)
            .arg("--data-dir")
            .arg(&root)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let ping = Request::new(Operation::GetStatus, None);
    for _ in 0..100 {
        if xray_supervisor::ipc::call(&root, &ping).await.is_ok() {
            break;
        }
        assert!(server.0.try_wait().unwrap().is_none());
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let json = format!("{{}}{}", " ".repeat(4 * 1024 * 1024 - 2));
    let first = Request::new(
        Operation::CreateProfile {
            profile_id: "a".into(),
            json: json.clone(),
        },
        None,
    );
    for request in [
        first.clone(),
        Request::new(
            Operation::CreateProfile {
                profile_id: "b".into(),
                json: json.clone(),
            },
            None,
        ),
    ] {
        let result = xray_supervisor::ipc::call(&root, &request)
            .await
            .unwrap()
            .result
            .unwrap();
        assert_eq!(result["json"].as_str().unwrap().len(), 4 * 1024 * 1024);
    }
    let summary = xray_supervisor::ipc::call(
        &root,
        &Request::new(
            Operation::GetOperation {
                request_id: first.request_id.clone(),
            },
            None,
        ),
    )
    .await
    .unwrap()
    .result
    .unwrap();
    assert_eq!(summary["state"], "Succeeded");
    assert_eq!(summary["command"], "CreateProfile");
    let digest = summary["details"]["sha256"].as_str().unwrap();
    let mut data = vec![];
    loop {
        let result = xray_supervisor::ipc::call(
            &root,
            &Request::new(
                Operation::GetOperationDetails {
                    request_id: first.request_id.clone(),
                    expected_hash: digest.into(),
                    offset: data.len(),
                    limit: 1048576,
                },
                None,
            ),
        )
        .await
        .unwrap()
        .result
        .unwrap();
        assert_eq!(result["offset"], data.len());
        data.extend(serde_json::from_value::<Vec<u8>>(result["bytes"].clone()).unwrap());
        if result["next_offset"].is_null() {
            break;
        }
    }
    assert_eq!(xray_model::hash(&data), digest);
    let details: Value = serde_json::from_slice(&data).unwrap();
    assert_eq!(details["request"]["command"]["payload"]["json"], json);
    assert_eq!(details["response"]["result"]["Ok"]["json"], json);
    let bad = xray_supervisor::ipc::call(
        &root,
        &Request::new(
            Operation::GetOperationDetails {
                request_id: first.request_id.clone(),
                expected_hash: "stale".into(),
                offset: 0,
                limit: 1,
            },
            None,
        ),
    )
    .await
    .unwrap()
    .result
    .unwrap_err();
    assert_eq!(bad.code, "OPERATION_DETAILS_CHANGED");
    for (after, expected, next) in [(None, "a", Some("a")), (Some("a"), "b", None)] {
        let page = xray_supervisor::ipc::call(
            &root,
            &Request::new(
                Operation::ListProfileSummaries {
                    after: after.map(str::to_owned),
                    limit: 1,
                },
                None,
            ),
        )
        .await
        .unwrap()
        .result
        .unwrap();
        assert_eq!(page["items"].as_array().unwrap().len(), 1);
        assert_eq!(page["items"][0]["id"], expected);
        assert_eq!(page["next_after"].as_str(), next);
        assert!(page["items"][0].get("json").is_none());
    }
    // The legacy aggregate endpoint returns a typed error, never an empty frame/EOF.
    let error = xray_supervisor::ipc::call(&root, &Request::new(Operation::ListProfiles, None))
        .await
        .unwrap()
        .result
        .unwrap_err();
    assert_eq!(error.code, "IPC_MESSAGE_TOO_LARGE");
    xray_supervisor::ipc::call(&root, &ping)
        .await
        .unwrap()
        .result
        .unwrap();
    xray_supervisor::ipc::call(&root, &Request::new(Operation::Shutdown, None))
        .await
        .unwrap()
        .result
        .unwrap();
    for _ in 0..100 {
        if let Some(exit) = server.0.try_wait().unwrap() {
            assert!(exit.success());
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("Supervisor did not shut down");
}
