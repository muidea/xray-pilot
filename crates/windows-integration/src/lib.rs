mod platform;
pub use platform::{SystemProxy, SystemStartup, winhttp_snapshot};
pub mod startup;

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use xray_config::{optional_json, write_json};
use xray_model::{Error, ProxySnapshot, Result, canonical_hash, id};

pub trait ProxyBackend: Send {
    fn read(&self) -> Result<ProxySnapshot>;
    fn write(&mut self, snapshot: &ProxySnapshot) -> Result<()>;
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum LeaseState {
    Owned,
    Released,
    Conflict,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Transition {
    pub before: ProxySnapshot,
    pub target: ProxySnapshot,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Lease {
    #[serde(default)]
    pub evidence_version: u32,
    pub id: String,
    pub root: ProxySnapshot,
    pub expected: ProxySnapshot,
    pub state: LeaseState,
    pub pending: Option<Transition>,
}
pub struct ProxyController<B: ProxyBackend> {
    pub backend: B,
    path: PathBuf,
    pub lease: Option<Lease>,
    recovery_error: Option<Error>,
}
impl<B: ProxyBackend> ProxyController<B> {
    pub fn open(backend: B, root: &Path) -> Result<Self> {
        let path = root.join("runtime/proxy-lease.json");
        let mut controller = Self {
            backend,
            path,
            lease: None,
            recovery_error: None,
        };
        if let Err(error) = controller.load() {
            controller.recovery_error = Some(Error::new(
                "PROXY_RECOVERY_REQUIRED",
                format!(
                    "Proxy lease evidence is unavailable or inconsistent ({}); preserve and repair the lease files before restarting",
                    error.code
                ),
            ));
        }
        Ok(controller)
    }
    fn evidence_path(&self) -> PathBuf {
        self.path.with_file_name("proxy-lease-evidence.json")
    }
    fn load(&mut self) -> Result<()> {
        let disk = optional_json::<Option<Lease>>(&self.path)?;
        let evidence = optional_json::<LeaseEvidence>(&self.evidence_path())?;
        if let Some(evidence) = evidence {
            evidence.verify()?;
            let actual = lease_hash(&disk)?;
            let target = canonical_hash(&evidence.lease)?;
            if evidence.required || (actual != target && actual != evidence.previous_hash) {
                return Err(Error::new(
                    "PROXY_RECOVERY_REQUIRED",
                    "Proxy lease does not match its independent evidence",
                ));
            }
            // The evidence is written first, before any Windows side effect. A matching
            // previous value proves an interrupted publication, including first acquisition.
            if actual != target {
                write_json(&self.path, &evidence.lease)?;
            }
            self.lease = evidence.lease;
        } else {
            self.lease = disk.flatten();
            if self.lease.as_ref().is_some_and(|l| l.evidence_version != 0) {
                return Err(Error::new(
                    "PROXY_RECOVERY_REQUIRED",
                    "Independent proxy evidence is missing",
                ));
            }
        }
        Ok(())
    }
    pub fn recovery_error(&self) -> Option<&Error> {
        self.recovery_error.as_ref()
    }
    /// Legacy Global commits also require a lease. Preserve this fact across safety stops.
    pub fn require_lease(&mut self) {
        if self.lease.is_none() && self.recovery_error.is_none() {
            self.recovery_error = Some(Error::new(
                "PROXY_RECOVERY_REQUIRED",
                "A Global commit has no proxy lease; restore the original lease evidence",
            ));
            let evidence = LeaseEvidence::new(None, "absent".into(), true);
            if let Err(error) = evidence.and_then(|e| write_json(&self.evidence_path(), &e)) {
                self.recovery_error = Some(error);
            }
        }
    }
    fn ensure_healthy(&self) -> Result<()> {
        self.recovery_error.clone().map_or(Ok(()), Err)
    }
    fn persist(&mut self) -> Result<()> {
        self.ensure_healthy()?;
        if let Some(lease) = &mut self.lease {
            lease.evidence_version = 1;
        }
        let result = (|| {
            let before = lease_hash(&optional_json::<Option<Lease>>(&self.path)?)?;
            let mut evidence = LeaseEvidence::new(self.lease.clone(), before, false)?;
            write_json(&self.evidence_path(), &evidence)?;
            write_json(&self.path, &self.lease)?;
            // Close the previous-value recovery window before callers may write Windows.
            evidence.previous_hash = canonical_hash(&self.lease)?;
            evidence.checksum = evidence.digest()?;
            write_json(&self.evidence_path(), &evidence)
        })();
        if let Err(error) = &result {
            self.recovery_error = Some(error.clone());
        }
        result
    }
    pub fn state(&self) -> String {
        if self.recovery_error.is_some() {
            return "RecoveryRequired".into();
        }
        self.lease
            .as_ref()
            .map(|l| format!("{:?}", l.state))
            .unwrap_or("Unmanaged".into())
    }
    pub fn snapshot(&self) -> Result<ProxySnapshot> {
        self.backend.read()
    }
    pub fn transition(&mut self, target: ProxySnapshot) -> Result<()> {
        self.ensure_healthy()?;
        let current = self.backend.read()?;
        if let Some(lease) = &self.lease
            && lease.state != LeaseState::Released
            && (lease.state == LeaseState::Conflict || current != lease.expected)
        {
            let detail = Self::conflict_detail(&current, lease);
            self.conflict()?;
            return Err(Error::new(
                "PROXY_LEASE_CONFLICT",
                format!("System proxy changed outside this manager ({detail})"),
            ));
        }
        if self
            .lease
            .as_ref()
            .is_none_or(|l| l.state == LeaseState::Released)
        {
            self.lease = Some(Lease {
                evidence_version: 1,
                id: id(),
                root: current.clone(),
                expected: current.clone(),
                state: LeaseState::Owned,
                pending: None,
            });
        }
        self.lease.as_mut().unwrap().pending = Some(Transition {
            before: current,
            target: target.clone(),
        });
        self.persist()?;
        self.backend.write(&target)?;
        if self.backend.read()? != target {
            return Err(Error::new(
                "PROXY_APPLY_FAILED",
                "System proxy readback differs from target",
            ));
        }
        self.lease.as_mut().unwrap().expected = target;
        self.persist()
    }
    pub fn finalize(&mut self) -> Result<()> {
        self.ensure_healthy()?;
        if let Some(lease) = self.lease.as_mut()
            && lease.state == LeaseState::Owned
        {
            lease.pending = None;
        }
        self.persist()
    }
    fn conflict(&mut self) -> Result<()> {
        if let Some(l) = &mut self.lease {
            l.state = LeaseState::Conflict;
        }
        self.persist()
    }
    fn snapshot_summary(s: &ProxySnapshot) -> String {
        format!(
            "flags={:#x} server={:?} bypass={:?} pac_url={:?}",
            s.flags, s.server, s.bypass, s.pac_url
        )
    }
    fn conflict_detail(current: &ProxySnapshot, lease: &Lease) -> String {
        format!(
            "state={:?}; current=({}); expected=({}); root=({})",
            lease.state,
            Self::snapshot_summary(current),
            Self::snapshot_summary(&lease.expected),
            Self::snapshot_summary(&lease.root)
        )
    }
    /// Resolves a `Conflict` lease by preserving the external system proxy as the new baseline without writing WinINet.
    pub fn acknowledge_conflict(&mut self) -> Result<ProxySnapshot> {
        self.ensure_healthy()?;
        let Some(lease) = self.lease.as_ref() else {
            return Err(Error::new(
                "INVALID_OPERATION",
                "No proxy lease is currently in conflict",
            ));
        };
        if lease.state != LeaseState::Conflict {
            return Err(Error::new(
                "INVALID_OPERATION",
                "Proxy lease is not in conflict state",
            ));
        }
        let current = self.backend.read()?;
        let l = self.lease.as_mut().unwrap();
        l.root = current.clone();
        l.expected = current.clone();
        l.pending = None;
        l.state = LeaseState::Released;
        self.persist()?;
        Ok(current)
    }
    /// Startup safety always releases an owned setting, including a write completed before bookkeeping.
    pub fn release(&mut self) -> Result<()> {
        self.ensure_healthy()?;
        let Some(lease) = self.lease.clone() else {
            return Ok(());
        };
        if lease.state == LeaseState::Released {
            return Ok(());
        }
        let current = self.backend.read()?;
        if current == lease.root {
            self.lease.as_mut().unwrap().state = LeaseState::Released;
            self.lease.as_mut().unwrap().pending = None;
            return self.persist();
        }
        let known = current == lease.expected
            || lease
                .pending
                .as_ref()
                .is_some_and(|p| current == p.target || current == p.before);
        if !known || lease.state == LeaseState::Conflict {
            let detail = Self::conflict_detail(&current, &lease);
            self.conflict()?;
            return Err(Error::new(
                "PROXY_LEASE_CONFLICT",
                format!("External system proxy settings preserved ({detail})"),
            ));
        }
        self.lease.as_mut().unwrap().pending = Some(Transition {
            before: current,
            target: lease.root.clone(),
        });
        self.persist()?;
        self.backend.write(&lease.root)?;
        if self.backend.read()? != lease.root {
            return Err(Error::new(
                "PROXY_RESTORE_FAILED",
                "Proxy restore readback failed",
            ));
        }
        let l = self.lease.as_mut().unwrap();
        l.expected = lease.root;
        l.pending = None;
        l.state = LeaseState::Released;
        self.persist()
    }
    pub fn compensate(
        &mut self,
        before: Option<Lease>,
        windows_before: ProxySnapshot,
    ) -> Result<()> {
        self.ensure_healthy()?;
        let current = self.backend.read()?;
        if current != windows_before {
            let recognized = self.lease.as_ref().is_some_and(|l| {
                current == l.expected || l.pending.as_ref().is_some_and(|p| current == p.target)
            });
            if !recognized {
                let detail = self
                    .lease
                    .as_ref()
                    .map(|l| Self::conflict_detail(&current, l))
                    .unwrap_or_else(|| Self::snapshot_summary(&current));
                self.conflict()?;
                return Err(Error::new(
                    "PROXY_LEASE_CONFLICT",
                    format!(
                        "External system proxy settings preserved during compensation ({detail})"
                    ),
                ));
            }
            // Persist compensation intent before the Windows write, preserving the root snapshot.
            if let Some(l) = &mut self.lease {
                l.pending = Some(Transition {
                    before: current,
                    target: windows_before.clone(),
                });
            }
            self.persist()?;
            self.backend.write(&windows_before)?;
            if self.backend.read()? != windows_before {
                return Err(Error::new(
                    "PROXY_RESTORE_FAILED",
                    "Compensation readback failed",
                ));
            }
        }
        self.lease = before;
        self.persist()
    }
}

#[derive(Serialize, Deserialize)]
struct LeaseEvidence {
    version: u32,
    lease: Option<Lease>,
    previous_hash: String,
    required: bool,
    checksum: String,
}
impl LeaseEvidence {
    fn new(lease: Option<Lease>, previous_hash: String, required: bool) -> Result<Self> {
        let mut evidence = Self {
            version: 1,
            lease,
            previous_hash,
            required,
            checksum: String::new(),
        };
        evidence.checksum = evidence.digest()?;
        Ok(evidence)
    }
    fn digest(&self) -> Result<String> {
        canonical_hash(&(
            self.version,
            &self.lease,
            &self.previous_hash,
            self.required,
        ))
    }
    fn verify(&self) -> Result<()> {
        if self.version != 1 || self.checksum != self.digest()? {
            return Err(Error::new(
                "PROXY_RECOVERY_REQUIRED",
                "Proxy evidence checksum/version is invalid",
            ));
        }
        Ok(())
    }
}
fn lease_hash(disk: &Option<Option<Lease>>) -> Result<String> {
    match disk {
        Some(lease) => canonical_hash(lease),
        None => Ok("absent".into()),
    }
}
/// Validate settled on-disk proxy evidence without reading or writing Windows settings.
pub fn check_offline_proxy_evidence(root: &Path) -> Result<()> {
    let disk = optional_json::<Option<Lease>>(&root.join("runtime/proxy-lease.json"))?;
    let witness = optional_json::<LeaseEvidence>(&root.join("runtime/proxy-lease-evidence.json"))?;
    if let Some(witness) = witness {
        witness.verify()?;
        if witness.required || lease_hash(&disk)? != canonical_hash(&witness.lease)? {
            return Err(Error::new(
                "MAINTENANCE_BLOCKED",
                "Proxy evidence requires recovery before maintenance",
            ));
        }
    } else if disk
        .as_ref()
        .and_then(|l| l.as_ref())
        .is_some_and(|l| l.evidence_version > 0)
    {
        return Err(Error::new(
            "MAINTENANCE_BLOCKED",
            "Independent proxy evidence is missing",
        ));
    }
    if disk
        .flatten()
        .is_some_and(|l| l.state != LeaseState::Released || l.pending.is_some())
    {
        return Err(Error::new(
            "MAINTENANCE_BLOCKED",
            "Stop Core and release the proxy lease before maintenance",
        ));
    }
    Ok(())
}
pub fn global(endpoint: std::net::SocketAddr) -> ProxySnapshot {
    ProxySnapshot {
        flags: 3,
        server: format!("http={endpoint};https={endpoint}"),
        bypass: "<local>".into(),
        pac_url: String::new(),
    }
}

/// WinINet PAC target. `PROXY_TYPE_DIRECT` must be present so a PAC result of
/// `DIRECT` is honored for hosts outside the proxy list; without it WinINet can
/// continue sending those hosts through the proxy. The PAC script itself
/// defaults to PROXY where it cannot decide, but an unreachable PAC host remains
/// a fail-open risk that must be covered by Windows validation.
pub fn pac(url: &str) -> ProxySnapshot {
    ProxySnapshot {
        flags: 1 | 4,
        server: String::new(),
        bypass: String::new(),
        pac_url: url.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Clone)]
    struct Fake(ProxySnapshot);
    impl ProxyBackend for Fake {
        fn read(&self) -> Result<ProxySnapshot> {
            Ok(self.0.clone())
        }
        fn write(&mut self, v: &ProxySnapshot) -> Result<()> {
            self.0 = v.clone();
            Ok(())
        }
    }
    #[test]
    fn repeated_switches_keep_root_and_external_changes_are_preserved() {
        let tmp = tempfile::tempdir().unwrap();
        let root = ProxySnapshot {
            flags: 5,
            pac_url: "https://corp/pac".into(),
            ..Default::default()
        };
        let mut proxy = ProxyController::open(Fake(root.clone()), tmp.path()).unwrap();
        proxy
            .transition(global("127.0.0.1:10809".parse().unwrap()))
            .unwrap();
        let id = proxy.lease.as_ref().unwrap().id.clone();
        proxy.finalize().unwrap();
        proxy
            .transition(global("127.0.0.1:20809".parse().unwrap()))
            .unwrap();
        assert_eq!(proxy.lease.as_ref().unwrap().id, id);
        proxy.release().unwrap();
        assert_eq!(proxy.backend.0, root);
        proxy
            .transition(global("127.0.0.1:10809".parse().unwrap()))
            .unwrap();
        proxy.backend.0 = ProxySnapshot {
            server: "external:80".into(),
            ..Default::default()
        };
        assert_eq!(proxy.release().unwrap_err().code, "PROXY_LEASE_CONFLICT");
        assert_eq!(proxy.backend.0.server, "external:80");
    }
    #[test]
    fn startup_recovers_write_before_finalize() {
        let tmp = tempfile::tempdir().unwrap();
        let root = ProxySnapshot {
            flags: 9,
            ..Default::default()
        };
        let mut proxy = ProxyController::open(Fake(root.clone()), tmp.path()).unwrap();
        proxy
            .transition(global("127.0.0.1:10809".parse().unwrap()))
            .unwrap();
        let mut recovered = ProxyController::open(proxy.backend, tmp.path()).unwrap();
        recovered.release().unwrap();
        recovered.finalize().unwrap();
        assert_eq!(recovered.backend.0, root);
        assert_eq!(
            recovered.lease.as_ref().unwrap().state,
            LeaseState::Released
        );
    }

    #[test]
    fn interrupted_lease_publication_uses_the_verified_intent() {
        let tmp = tempfile::tempdir().unwrap();
        let mut proxy = ProxyController::open(Fake(ProxySnapshot::default()), tmp.path()).unwrap();
        proxy
            .transition(global("127.0.0.1:10809".parse().unwrap()))
            .unwrap();
        let before = lease_hash(&optional_json::<Option<Lease>>(&proxy.path).unwrap()).unwrap();
        let mut next = proxy.lease.clone().unwrap();
        next.pending = Some(Transition {
            before: next.expected.clone(),
            target: global("127.0.0.1:20809".parse().unwrap()),
        });
        write_json(
            &proxy.evidence_path(),
            &LeaseEvidence::new(Some(next), before, false).unwrap(),
        )
        .unwrap();
        let mut reopened = ProxyController::open(proxy.backend, tmp.path()).unwrap();
        assert!(reopened.recovery_error().is_none());
        reopened.release().unwrap();
        assert_eq!(reopened.backend.0, ProxySnapshot::default());
    }

    #[test]
    fn normal_release_and_compensation_to_unmanaged_are_not_missing_evidence() {
        let tmp = tempfile::tempdir().unwrap();
        let mut proxy = ProxyController::open(Fake(ProxySnapshot::default()), tmp.path()).unwrap();
        assert_eq!(proxy.state(), "Unmanaged");
        proxy
            .transition(global("127.0.0.1:10809".parse().unwrap()))
            .unwrap();
        proxy.compensate(None, ProxySnapshot::default()).unwrap();
        let mut proxy = ProxyController::open(proxy.backend, tmp.path()).unwrap();
        assert!(proxy.recovery_error().is_none());
        assert_eq!(proxy.state(), "Unmanaged");
        proxy
            .transition(global("127.0.0.1:20809".parse().unwrap()))
            .unwrap();
        proxy.release().unwrap();
        let proxy = ProxyController::open(proxy.backend, tmp.path()).unwrap();
        assert!(proxy.recovery_error().is_none());
        assert_eq!(proxy.state(), "Released");
    }

    #[test]
    fn acknowledging_conflict_preserves_external_proxy_and_allows_reopening() {
        let tmp = tempfile::tempdir().unwrap();
        let mut proxy = ProxyController::open(Fake(ProxySnapshot::default()), tmp.path()).unwrap();
        proxy
            .transition(global("127.0.0.1:10809".parse().unwrap()))
            .unwrap();
        let external = ProxySnapshot {
            flags: 3,
            server: "external:8888".into(),
            bypass: "<local>".into(),
            pac_url: String::new(),
        };
        proxy.backend.0 = external.clone();
        let err = proxy.release().unwrap_err();
        assert_eq!(err.code, "PROXY_LEASE_CONFLICT");
        assert!(err.message.contains("external:8888"));
        assert_eq!(proxy.state(), "Conflict");

        let preserved = proxy.acknowledge_conflict().unwrap();
        assert_eq!(preserved, external);
        assert_eq!(proxy.backend.0, external);
        assert_eq!(proxy.state(), "Released");

        let mut reopened = ProxyController::open(proxy.backend, tmp.path()).unwrap();
        assert!(reopened.recovery_error().is_none());
        assert_eq!(reopened.state(), "Released");
        reopened
            .transition(global("127.0.0.1:20809".parse().unwrap()))
            .unwrap();
        reopened.release().unwrap();
        assert_eq!(reopened.backend.0, external);
    }
}
