use crate::{Supervisor, default_root};
use std::path::Path;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
#[cfg(windows)]
use xray_model::hash;
use xray_model::{Error, Request, Response, Result};

const MAX_MESSAGE: usize = 8 * 1024 * 1024;
pub fn endpoint(root: &Path) -> String {
    #[cfg(windows)]
    {
        format!(
            r"\\.\pipe\xray-pilot-{}",
            &hash(
                format!(
                    "{}:{}",
                    xray_config::security::principal().unwrap_or_default(),
                    root.display()
                )
                .as_bytes()
            )[..32]
        )
    }
    #[cfg(not(windows))]
    {
        root.join("runtime/supervisor.sock")
            .to_string_lossy()
            .into_owned()
    }
}
async fn receive<R: AsyncRead + Unpin, T: serde::de::DeserializeOwned>(io: &mut R) -> Result<T> {
    let size = io.read_u32_le().await? as usize;
    if size > MAX_MESSAGE {
        return Err(Error::new("IPC_MESSAGE_TOO_LARGE", "Message exceeds 8 MiB"));
    }
    let mut bytes = vec![0; size];
    io.read_exact(&mut bytes).await?;
    Ok(serde_json::from_slice(&bytes)?)
}
async fn send_frame<W: AsyncWrite + Unpin>(io: &mut W, bytes: &[u8]) -> Result<()> {
    if bytes.len() > MAX_MESSAGE {
        return Err(Error::new("IPC_MESSAGE_TOO_LARGE", "Message exceeds 8 MiB"));
    }
    io.write_u32_le(bytes.len() as u32).await?;
    io.write_all(bytes).await?;
    io.flush().await?;
    Ok(())
}
async fn send<W: AsyncWrite + Unpin, T: serde::Serialize>(io: &mut W, value: &T) -> Result<()> {
    send_frame(io, &serde_json::to_vec(value)?).await
}
// Encode exactly once, off the async workers. The admission slot remains held
// until encoding finishes, even if the connection's I/O timeout cancels its wait.
async fn send_response<W: AsyncWrite + Unpin>(
    io: &mut W,
    response: Response,
    slot: std::sync::Arc<tokio::sync::OwnedSemaphorePermit>,
) -> Result<()> {
    let bytes = tokio::task::spawn_blocking(move || -> Result<Vec<u8>> {
        let _slot = slot;
        let bytes = serde_json::to_vec(&response)?;
        if bytes.len() <= MAX_MESSAGE { return Ok(bytes); }
        // Serialize the fallback before writing any header; oversized replies are never EOF.
        Ok(serde_json::to_vec(&Response {
            request_id: response.request_id,
            result: Err(Error::new("IPC_MESSAGE_TOO_LARGE",
                "Response exceeds 8 MiB; a write may already have committed. Query its request ID; use paged summaries or operation details")),
        })?)
    }).await.map_err(|_| Error::new("IPC_SERIALIZATION_FAILED", "Response encoding failed"))??;
    send_frame(io, &bytes).await
}
pub async fn call(root: &Path, request: &Request) -> Result<Response> {
    let root = std::fs::canonicalize(root)?;
    #[cfg(windows)]
    let mut connection = {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(90);
        let client = loop {
            match tokio::net::windows::named_pipe::ClientOptions::new().open(endpoint(&root)) {
                Ok(client) => break client,
                Err(e)
                    if e.raw_os_error() == Some(231) && tokio::time::Instant::now() < deadline =>
                {
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }
                Err(e) => return Err(e.into()),
            }
        };
        use std::os::windows::io::AsRawHandle;
        verify_peer(client.as_raw_handle(), false)?;
        client
    };
    #[cfg(not(windows))]
    let mut connection = tokio::net::UnixStream::connect(endpoint(&root)).await?;
    tokio::time::timeout(std::time::Duration::from_secs(90), async {
        send(&mut connection, request).await?;
        let response = receive(&mut connection).await?;
        // DisconnectNamedPipe discards unread data; acknowledge the fully received response.
        #[cfg(windows)]
        {
            connection.write_u8(1).await?;
            connection.flush().await?;
        }
        Ok(response)
    })
    .await
    .map_err(|_| {
        Error::new(
            "IPC_TIMEOUT",
            "Request timed out; query its request ID before retrying",
        )
    })?
}

#[cfg(windows)]
fn verify_peer(handle: std::os::windows::io::RawHandle, is_client: bool) -> Result<()> {
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::{
            Pipes::{GetNamedPipeClientProcessId, GetNamedPipeServerProcessId},
            Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
        },
    };
    unsafe {
        let mut pid = 0;
        let ok = if is_client {
            GetNamedPipeClientProcessId(handle, &mut pid)
        } else {
            GetNamedPipeServerProcessId(handle, &mut pid)
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return Err(std::io::Error::last_os_error().into());
        }
        let identity = xray_config::security::process_principal(process);
        CloseHandle(process);
        if identity? != xray_config::security::principal()? {
            return Err(Error::new(
                "IPC_ACCESS_DENIED",
                "Pipe peer is not the current user",
            ));
        }
        Ok(())
    }
}

const MAX_CONNECTIONS: usize = 16;
const IO_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
struct Dispatch {
    request: Request,
    response: tokio::sync::oneshot::Sender<Response>,
}

// Only this task owns the state machine. Connection tasks never hold it while waiting for I/O.
pub async fn serve(root: &Path) -> Result<()> {
    serve_supervisor(Supervisor::open(root).await?).await
}
async fn connection<S: AsyncRead + AsyncWrite + Unpin>(
    io: &mut S,
    dispatch: tokio::sync::mpsc::Sender<Dispatch>,
    slot: std::sync::Arc<tokio::sync::OwnedSemaphorePermit>,
) {
    let Ok(Ok(request)) = tokio::time::timeout(IO_TIMEOUT, receive::<_, Request>(io)).await else {
        return;
    };
    let request_id = request.request_id.clone();
    let (tx, rx) = tokio::sync::oneshot::channel();
    let response = match dispatch.try_send(Dispatch {
        request,
        response: tx,
    }) {
        Ok(()) => match rx.await {
            Ok(response) => response,
            Err(_) => return,
        },
        Err(_) => Response {
            request_id,
            result: Err(Error::new(
                "IPC_BUSY",
                "Supervisor request queue is full or closing; this request was not accepted",
            )),
        },
    };
    if matches!(
        tokio::time::timeout(IO_TIMEOUT, send_response(io, response, slot)).await,
        Ok(Ok(()))
    ) {
        #[cfg(windows)]
        let _ = tokio::time::timeout(IO_TIMEOUT, io.read_u8()).await;
    }
}
async fn serve_supervisor<B: windows_integration::ProxyBackend + 'static>(
    mut supervisor: Supervisor<B>,
) -> Result<()> {
    let mut interval = tokio::time::interval(std::time::Duration::from_millis(300));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let slots = std::sync::Arc::new(tokio::sync::Semaphore::new(MAX_CONNECTIONS));
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Dispatch>(MAX_CONNECTIONS);
    let mut connections = tokio::task::JoinSet::new();
    #[cfg(not(windows))]
    let path = endpoint(&supervisor.store.root);
    #[cfg(not(windows))]
    let listener = {
        let _ = std::fs::remove_file(&path);
        tokio::net::UnixListener::bind(&path)?
    };
    #[cfg(windows)]
    let name = endpoint(&supervisor.store.root);
    #[cfg(windows)]
    let create_pipe = |first| -> Result<tokio::net::windows::named_pipe::NamedPipeServer> {
        use windows_sys::Win32::{Foundation::LocalFree, Security::SECURITY_ATTRIBUTES};
        let descriptor = xray_config::security::descriptor()?;
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        };
        let created = unsafe {
            tokio::net::windows::named_pipe::ServerOptions::new()
                .first_pipe_instance(first)
                .reject_remote_clients(true)
                .create_with_security_attributes_raw(
                    &name,
                    (&attributes as *const SECURITY_ATTRIBUTES)
                        .cast_mut()
                        .cast(),
                )
        };
        unsafe {
            LocalFree(descriptor);
        }
        Ok(created?)
    };
    #[cfg(windows)]
    let mut server = create_pipe(true)?;
    while !supervisor.shutdown {
        tokio::select! {
            biased;
            _ = interval.tick() => supervisor.tick().await,
            Some(message) = rx.recv() => {
                let response = supervisor.handle(message.request).await;
                let _ = message.response.send(response);
            }
            Some(_) = connections.join_next(), if !connections.is_empty() => {},
            accepted = async {
                #[cfg(not(windows))]
                { Ok::<_, Error>(listener.accept().await?.0) }
                #[cfg(windows)]
                {
                    server.connect().await?;
                    // Own the pipe name continuously; each accepted client gets a fresh instance.
                    Ok::<_, Error>(std::mem::replace(&mut server, create_pipe(false)?))
                }
            } => {
                let mut io = accepted?;
                let Ok(permit) = slots.clone().try_acquire_owned() else {
                    // Bounded admission also bounds request buffers and response-waiting tasks.
                    continue;
                };
                let dispatch = tx.clone();
                connections.spawn(async move {
                    let slot = std::sync::Arc::new(permit);
                    #[cfg(windows)]
                    {
                        use std::os::windows::io::AsRawHandle;
                        if verify_peer(io.as_raw_handle(), true).is_err() { return; }
                    }
                    connection(&mut io, dispatch, slot.clone()).await;
                    #[cfg(windows)]
                    let _ = io.disconnect();
                });
            }
        }
    }
    rx.close();
    drop(rx); // Requests still queued were never accepted by the coordinator.
    drop(tx);
    // Allow the shutdown response to drain, without waiting indefinitely for idle clients or ACKs.
    let _ = tokio::time::timeout(IO_TIMEOUT, async {
        while connections.join_next().await.is_some() {}
    })
    .await;
    connections.abort_all();
    while connections.join_next().await.is_some() {}
    #[cfg(not(windows))]
    let _ = std::fs::remove_file(path);
    Ok(())
}
pub fn data_arg() -> Result<std::path::PathBuf> {
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--data-dir" {
            return args
                .next()
                .map(Into::into)
                .ok_or_else(|| Error::new("INVALID_ARGUMENT", "--data-dir needs a path"));
        }
    }
    default_root()
}

#[cfg(test)]
#[path = "ipc_tests.rs"]
mod tests;
