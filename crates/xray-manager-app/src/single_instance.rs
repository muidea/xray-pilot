use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(not(windows), allow(dead_code))]
pub enum Signal {
    Open,
    Ping,
}

impl Signal {
    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn as_bytes(self) -> &'static [u8] {
        match self {
            Self::Open => b"OPEN\n",
            Self::Ping => b"PING\n",
        }
    }

    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn parse(line: &str) -> Option<Self> {
        match line.trim() {
            "OPEN" => Some(Self::Open),
            "PING" => Some(Self::Ping),
            _ => None,
        }
    }
}

#[derive(Clone)]
pub struct ActivationHandle {
    open_requested: Arc<AtomicBool>,
    wakeup: Arc<Mutex<Option<crate::wakeup::Wakeup>>>,
}

impl ActivationHandle {
    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn new() -> Self {
        Self {
            open_requested: Arc::new(AtomicBool::new(false)),
            wakeup: Arc::new(Mutex::new(None)),
        }
    }

    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn attach_wakeup(&self, wakeup: crate::wakeup::Wakeup) {
        if let Ok(mut slot) = self.wakeup.lock() {
            *slot = Some(wakeup.clone());
        }
        if self.open_requested.load(Ordering::Acquire) {
            wakeup.request();
        }
    }

    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn request_open(&self) {
        self.open_requested.store(true, Ordering::Release);
        if let Ok(slot) = self.wakeup.lock()
            && let Some(wakeup) = slot.as_ref()
        {
            wakeup.request();
        }
    }

    pub fn take_open(&self) -> bool {
        self.open_requested.swap(false, Ordering::AcqRel)
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
pub async fn handle_connection<S: AsyncRead + AsyncWrite + Unpin>(
    io: S,
    activation: &ActivationHandle,
) -> std::io::Result<()> {
    let (reader, mut writer) = tokio::io::split(io);
    let mut reader = BufReader::new(reader);
    let mut line = String::new();
    let read = tokio::time::timeout(Duration::from_secs(2), async {
        // Bound line length without allocating unbounded memory on malformed input.
        let mut limited = reader.take_until_newline();
        limited.read_line(&mut line).await
    })
    .await;
    let reply: &[u8] = match read {
        Ok(Ok(_)) => match Signal::parse(&line) {
            Some(Signal::Open) => {
                activation.request_open();
                b"OK\n"
            }
            Some(Signal::Ping) => b"OK\n",
            None => b"ERR\n",
        },
        _ => b"ERR\n",
    };
    let _ = tokio::time::timeout(Duration::from_secs(2), writer.write_all(reply)).await;
    let _ = tokio::time::timeout(Duration::from_secs(2), writer.flush()).await;
    Ok(())
}

trait BoundedLineExt: AsyncBufReadExt + Unpin {
    fn take_until_newline(&mut self) -> BoundedReader<'_, Self> {
        BoundedReader {
            inner: self,
            remaining: 64,
        }
    }
}

impl<T: AsyncBufReadExt + Unpin> BoundedLineExt for T {}

struct BoundedReader<'a, R: ?Sized> {
    inner: &'a mut R,
    remaining: usize,
}

impl<R: AsyncBufReadExt + Unpin + ?Sized> BoundedReader<'_, R> {
    async fn read_line(&mut self, out: &mut String) -> std::io::Result<usize> {
        let mut bytes = Vec::with_capacity(16);
        while bytes.len() < self.remaining {
            let mut one = [0u8; 1];
            let n = tokio::io::AsyncReadExt::read(self.inner, &mut one).await?;
            if n == 0 {
                break;
            }
            bytes.push(one[0]);
            if one[0] == b'\n' {
                break;
            }
        }
        let text = String::from_utf8_lossy(&bytes);
        out.push_str(&text);
        Ok(bytes.len())
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
pub async fn send_signal<S: AsyncRead + AsyncWrite + Unpin>(
    mut io: S,
    signal: Signal,
) -> std::io::Result<()> {
    tokio::time::timeout(Duration::from_secs(2), async {
        io.write_all(signal.as_bytes()).await?;
        io.flush().await?;
        let mut reader = BufReader::new(io);
        let mut reply = String::new();
        reader.take_until_newline().read_line(&mut reply).await?;
        if reply.trim() == "OK" {
            Ok(())
        } else {
            Err(std::io::Error::other("Unexpected single-instance reply"))
        }
    })
    .await
    .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "Single-instance timeout"))?
}

#[cfg(windows)]
mod platform {
    use super::{ActivationHandle, Signal, handle_connection, send_signal};
    use std::path::Path;
    use std::time::Duration;
    use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeServer, ServerOptions};
    use tokio::sync::watch;

    pub fn endpoint(root: &Path) -> String {
        format!(
            r"\\.\pipe\xray-pilot-gui-{}",
            &xray_model::hash(
                format!(
                    "{}:{}",
                    xray_config::security::principal().unwrap_or_default(),
                    root.display()
                )
                .as_bytes()
            )[..32]
        )
    }

    fn create_pipe(name: &str, first: bool) -> std::io::Result<NamedPipeServer> {
        use windows_sys::Win32::{Foundation::LocalFree, Security::SECURITY_ATTRIBUTES};
        let descriptor = xray_config::security::descriptor()
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        };
        let created = unsafe {
            ServerOptions::new()
                .first_pipe_instance(first)
                .reject_remote_clients(true)
                .create_with_security_attributes_raw(
                    name,
                    (&attributes as *const SECURITY_ATTRIBUTES)
                        .cast_mut()
                        .cast(),
                )
        };
        unsafe {
            LocalFree(descriptor);
        }
        created
    }

    fn is_pipe_claimed(err: &std::io::Error) -> bool {
        matches!(
            err.kind(),
            std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::AlreadyExists
        ) || matches!(err.raw_os_error(), Some(5 | 80 | 183 | 231))
    }

    pub struct InstanceGuard {
        activation: ActivationHandle,
        shutdown: Option<watch::Sender<bool>>,
        worker: Option<std::thread::JoinHandle<()>>,
    }

    impl InstanceGuard {
        pub fn activation(&self) -> ActivationHandle {
            self.activation.clone()
        }
    }

    impl Drop for InstanceGuard {
        fn drop(&mut self) {
            if let Some(tx) = self.shutdown.take() {
                let _ = tx.send(true);
            }
            if let Some(handle) = self.worker.take() {
                let _ = handle.join();
            }
        }
    }

    pub fn acquire_or_activate(
        root: &Path,
        background: bool,
    ) -> std::io::Result<Option<InstanceGuard>> {
        let name = endpoint(root);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let signal = if background {
            Signal::Ping
        } else {
            Signal::Open
        };

        let server = runtime.block_on(async {
            for attempt in 0..80 {
                match create_pipe(&name, true) {
                    Ok(server) => return Ok(Some(server)),
                    Err(e) if !is_pipe_claimed(&e) => return Err(e),
                    Err(e) => {
                        match ClientOptions::new().open(&name) {
                            Ok(client) => {
                                if send_signal(client, signal).await.is_ok() {
                                    return Ok(None);
                                }
                            }
                            Err(ce) if ce.kind() == std::io::ErrorKind::NotFound => {}
                            Err(_) => {}
                        }
                        if attempt == 79 {
                            return Err(e);
                        }
                        tokio::time::sleep(Duration::from_millis(25)).await;
                    }
                }
            }
            Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "Another Xray Pilot GUI instance owns the pipe",
            ))
        })?;

        let Some(mut server) = server else {
            return Ok(None);
        };

        let activation = ActivationHandle::new();
        let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
        let loop_activation = activation.clone();
        let worker = std::thread::spawn(move || {
            runtime.block_on(async move {
                loop {
                    tokio::select! {
                        changed = shutdown_rx.changed() => {
                            if changed.is_err() || *shutdown_rx.borrow() {
                                break;
                            }
                        }
                        connected = server.connect() => {
                            if connected.is_err() {
                                break;
                            }
                            let Ok(next) = create_pipe(&name, false) else {
                                break;
                            };
                            let stream = std::mem::replace(&mut server, next);
                            let activation = loop_activation.clone();
                            tokio::spawn(async move {
                                let _ = handle_connection(stream, &activation).await;
                            });
                        }
                    }
                }
            });
        });

        Ok(Some(InstanceGuard {
            activation,
            shutdown: Some(shutdown_tx),
            worker: Some(worker),
        }))
    }
}

#[cfg(windows)]
pub use platform::acquire_or_activate;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn open_signal_requests_activation_and_ping_stays_quiet() {
        let activation = ActivationHandle::new();
        let (client, server) = tokio::io::duplex(128);
        let server_activation = activation.clone();
        let srv = tokio::spawn(async move {
            handle_connection(server, &server_activation).await.unwrap();
        });
        send_signal(client, Signal::Ping).await.unwrap();
        srv.await.unwrap();
        assert!(!activation.take_open());

        let (client, server) = tokio::io::duplex(128);
        let server_activation = activation.clone();
        let srv = tokio::spawn(async move {
            handle_connection(server, &server_activation).await.unwrap();
        });
        send_signal(client, Signal::Open).await.unwrap();
        srv.await.unwrap();
        assert!(activation.take_open());
        assert!(!activation.take_open());
    }

    #[tokio::test]
    async fn malformed_signal_returns_error_without_activating() {
        let activation = ActivationHandle::new();
        let (mut client, server) = tokio::io::duplex(128);
        let server_activation = activation.clone();
        let srv = tokio::spawn(async move {
            handle_connection(server, &server_activation).await.unwrap();
        });
        client.write_all(b"UNKNOWN\n").await.unwrap();
        client.flush().await.unwrap();
        let mut reply = String::new();
        BufReader::new(client)
            .take_until_newline()
            .read_line(&mut reply)
            .await
            .unwrap();
        srv.await.unwrap();
        assert_eq!(reply.trim(), "ERR");
        assert!(!activation.take_open());
    }
}
