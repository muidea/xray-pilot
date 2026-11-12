//! Local protocol readiness. HTTP uses a loopback target; SOCKS stops after authentication.
use crate::*;
use base64::Engine;
use serde_json::Value;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Http,
    Socks,
    Mixed,
}
#[derive(Clone)]
pub struct Listener {
    endpoint: SocketAddr,
    kind: Kind,
    account: Option<(String, String)>,
}
/// Unsupported native listeners are reported rather than mistaken for a completed protocol check.
pub fn listener_plan(config: &Value) -> Result<(Vec<Listener>, Vec<String>)> {
    let mut plan = vec![];
    let mut notes = vec![];
    for (index, inbound) in config
        .get("inbounds")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
    {
        let protocol = inbound["protocol"].as_str().unwrap_or("");
        let network = inbound
            .pointer("/streamSettings/network")
            .and_then(Value::as_str)
            .unwrap_or("tcp");
        let security = inbound
            .pointer("/streamSettings/security")
            .and_then(Value::as_str)
            .unwrap_or("none");
        let port = inbound["port"].as_u64().filter(|p| (1..=65535).contains(p));
        let ip = inbound["listen"]
            .as_str()
            .unwrap_or("0.0.0.0")
            .parse::<IpAddr>()
            .ok();
        let wrapped = ["tcpSettings", "rawSettings"].iter().any(|key| {
            let Some(settings) = inbound.pointer(&format!("/streamSettings/{key}")) else {
                return false;
            };
            settings
                .get("acceptProxyProtocol")
                .is_some_and(|v| v != false)
                || settings
                    .pointer("/header/type")
                    .is_some_and(|v| v != "none")
        });
        if wrapped
            || !matches!(protocol, "http" | "socks" | "mixed")
            || !matches!(network, "tcp" | "raw")
            || security != "none"
            || port.is_none()
            || ip.is_none()
        {
            notes.push(format!("WARNING Local readiness does not cover inbound {index}; Core validation and process survival do not prove protocol readiness"));
            continue;
        }
        let mut ip = ip.unwrap();
        if ip.is_unspecified() {
            ip = if ip.is_ipv4() {
                IpAddr::V4(Ipv4Addr::LOCALHOST)
            } else {
                IpAddr::V6(Ipv6Addr::LOCALHOST)
            };
        }
        // Probe only addresses on this machine. Arbitrary/non-loopback bind addresses remain explicit limitations.
        if !ip.is_loopback() {
            notes.push(format!(
                "WARNING Local readiness does not cover non-loopback inbound {index}"
            ));
            continue;
        }
        let account = inbound
            .pointer("/settings/accounts")
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .and_then(|a| {
                Some((
                    a["user"].as_str()?.to_owned(),
                    a["pass"].as_str()?.to_owned(),
                ))
            });
        let auth = inbound
            .pointer("/settings/auth")
            .and_then(Value::as_str)
            .unwrap_or("noauth");
        let account = if matches!(protocol, "socks" | "mixed") && auth == "noauth" {
            None
        } else {
            account
        };
        if matches!(protocol, "socks" | "mixed") && auth == "password" && account.is_none() {
            return Err(Error::new(
                "LOCAL_PROXY_NOT_READY",
                format!("Inbound {index} requires an account for local readiness"),
            ));
        }
        plan.push(Listener {
            endpoint: SocketAddr::new(ip, port.unwrap() as u16),
            kind: match protocol {
                "http" => Kind::Http,
                "socks" => Kind::Socks,
                _ => Kind::Mixed,
            },
            account,
        });
    }
    Ok((plan, notes))
}
fn invalid() -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        "Listener did not complete the expected local proxy handshake",
    )
}
async fn socks_once(listener: &Listener) -> std::io::Result<()> {
    let mut stream = TcpStream::connect(listener.endpoint).await?;
    let method = if listener.account.is_some() { 2 } else { 0 };
    stream.write_all(&[5, 1, method]).await?;
    let mut reply = [0; 2];
    stream.read_exact(&mut reply).await?;
    if reply != [5, method] {
        return Err(invalid());
    }
    if let Some((user, pass)) = &listener.account {
        if user.is_empty() || user.len() > 255 || pass.len() > 255 {
            return Err(invalid());
        }
        let mut auth = vec![1, user.len() as u8];
        auth.extend(user.as_bytes());
        auth.push(pass.len() as u8);
        auth.extend(pass.as_bytes());
        stream.write_all(&auth).await?;
        stream.read_exact(&mut reply).await?;
        if reply != [1, 0] {
            return Err(invalid());
        }
    }
    // Deliberately stop after authentication. No target connection is requested.
    Ok(())
}
async fn http_once(listener: &Listener) -> std::io::Result<()> {
    let mut stream = TcpStream::connect(listener.endpoint).await?;
    let authorization = listener
        .account
        .as_ref()
        .map(|(user, pass)| {
            format!(
                "Proxy-Authorization: Basic {}\r\n",
                base64::engine::general_purpose::STANDARD.encode(format!("{user}:{pass}"))
            )
        })
        .unwrap_or_default();
    stream
        .write_all(
            format!("CONNECT 127.0.0.1:0 HTTP/1.1\r\nHost: 127.0.0.1:0\r\n{authorization}\r\n")
                .as_bytes(),
        )
        .await?;
    let mut reply = [0; 13];
    stream.read_exact(&mut reply).await?;
    if &reply != b"HTTP/1.1 200 " && &reply != b"HTTP/1.0 200 " {
        return Err(invalid());
    }
    Ok(())
}
async fn once(listener: &Listener) -> std::io::Result<()> {
    match listener.kind {
        Kind::Http => http_once(listener).await,
        Kind::Socks => socks_once(listener).await,
        Kind::Mixed => match socks_once(listener).await {
            Ok(()) => Ok(()),
            // Xray's mixed inbound accepts both; a non-SOCKS reply means try HTTP
            // on a fresh connection rather than treating one protocol as failure.
            Err(_) => http_once(listener).await,
        },
    }
}
async fn probe(listener: Listener) -> Result<()> {
    timeout(Duration::from_secs(3), async {
        loop {
            match once(&listener).await {
                Ok(()) => return Ok(()),
                Err(e) if e.kind() != std::io::ErrorKind::InvalidData => {
                    tokio::time::sleep(Duration::from_millis(50)).await
                }
                Err(_) => {
                    return Err(Error::new(
                        "LOCAL_PROXY_NOT_READY",
                        "Local listener returned an incompatible proxy handshake",
                    ));
                }
            }
        }
    })
    .await
    .map_err(|_| Error::new("LOCAL_PROXY_NOT_READY", "Local proxy readiness timed out"))?
}
pub(super) async fn wait_all(listeners: Vec<Listener>) -> Result<()> {
    timeout(Duration::from_secs(5), async {
        for group in listeners.chunks(16) {
            let mut tasks = tokio::task::JoinSet::new();
            for listener in group {
                tasks.spawn(probe(listener.clone()));
            }
            while let Some(result) = tasks.join_next().await {
                result
                    .map_err(|_| Error::new("LOCAL_PROXY_NOT_READY", "Readiness task failed"))??;
            }
        }
        Ok(())
    })
    .await
    .map_err(|_| {
        Error::new(
            "LOCAL_PROXY_NOT_READY",
            "Listener readiness exceeded its total deadline",
        )
    })?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn socks_authentication_is_checked_without_connecting_to_any_target() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            let mut greeting = [0; 3];
            s.read_exact(&mut greeting).await.unwrap();
            assert_eq!(greeting, [5, 1, 2]);
            s.write_all(&[5, 2]).await.unwrap();
            let mut auth = [0; 6];
            s.read_exact(&mut auth).await.unwrap();
            assert_eq!(auth, [1, 1, b'u', 2, b'p', b'w']);
            s.write_all(&[1, 0]).await.unwrap();
            assert!(s.read_u8().await.is_err());
        });
        probe(Listener {
            endpoint: address,
            kind: Kind::Socks,
            account: Some(("u".into(), "pw".into())),
        })
        .await
        .unwrap();
        server.await.unwrap();
    }
    #[tokio::test]
    async fn wrong_service_is_rejected() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            let mut hello = [0; 3];
            s.read_exact(&mut hello).await.unwrap();
            s.write_all(&[4, 0]).await.unwrap();
        });
        assert_eq!(
            probe(Listener {
                endpoint: address,
                kind: Kind::Socks,
                account: None
            })
            .await
            .unwrap_err()
            .code,
            "LOCAL_PROXY_NOT_READY"
        );
        server.await.unwrap();
    }
    #[tokio::test]
    async fn cancelling_a_group_closes_pending_handshakes() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut hello = [0; 3];
            stream.read_exact(&mut hello).await.unwrap();
            ready_tx.send(()).unwrap();
            assert!(
                timeout(Duration::from_secs(1), stream.read_u8())
                    .await
                    .unwrap()
                    .is_err()
            );
        });
        let waiting = tokio::spawn(wait_all(vec![Listener {
            endpoint: address,
            kind: Kind::Socks,
            account: None,
        }]));
        ready_rx.await.unwrap();
        waiting.abort();
        let _ = waiting.await;
        server.await.unwrap();
    }
    #[test]
    fn planning_is_local_and_unsupported_transports_remain_explicit() {
        let (plan,notes)=listener_plan(&serde_json::json!({"inbounds":[{"protocol":"socks","port":10808},{"protocol":"http","port":10809,"listen":"::"},{"protocol":"mixed","port":10807},{"protocol":"socks","port":10810,"streamSettings":{"security":"tls"}},{"protocol":"custom","port":1234}]})).unwrap();
        assert_eq!(plan.len(), 3);
        assert!(plan.iter().all(|p| p.endpoint.ip().is_loopback()));
        assert_eq!(notes.len(), 2);
        let (plan, notes)=listener_plan(&serde_json::json!({"inbounds":[{"protocol":"http","port":10809,"streamSettings":{"rawSettings":{"acceptProxyProtocol":true}}},{"protocol":"socks","port":10808,"streamSettings":{"tcpSettings":{"header":{"type":"http"}}}}]})).unwrap();
        assert!(plan.is_empty());
        assert_eq!(notes.len(), 2);
    }
}

#[cfg(test)]
mod mixed_tests {
    use super::*;
    #[tokio::test]
    async fn mixed_listener_accepts_socks_and_http_clients() {
        // A SOCKS-only reply is enough for the mixed probe.
        let socks = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let socks_addr = socks.local_addr().unwrap();
        let socks_server = tokio::spawn(async move {
            let (mut s, _) = socks.accept().await.unwrap();
            let mut greeting = [0; 3];
            s.read_exact(&mut greeting).await.unwrap();
            s.write_all(&[5, 0]).await.unwrap();
        });
        probe(Listener {
            endpoint: socks_addr,
            kind: Kind::Mixed,
            account: None,
        })
        .await
        .unwrap();
        socks_server.await.unwrap();

        // First reply rejects SOCKS authentication, so the mixed probe retries
        // on a fresh connection as HTTP.
        let http = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let http_addr = http.local_addr().unwrap();
        let http_server = tokio::spawn(async move {
            let (mut s, _) = http.accept().await.unwrap();
            let mut greeting = [0; 3];
            s.read_exact(&mut greeting).await.unwrap();
            s.write_all(&[5, 2]).await.unwrap();
            let (mut s, _) = http.accept().await.unwrap();
            let mut request = [0; 64];
            let n = s.read(&mut request).await.unwrap();
            assert!(String::from_utf8_lossy(&request[..n]).starts_with("CONNECT "));
            s.write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
                .await
                .unwrap();
        });
        probe(Listener {
            endpoint: http_addr,
            kind: Kind::Mixed,
            account: None,
        })
        .await
        .unwrap();
        http_server.await.unwrap();
    }
}
