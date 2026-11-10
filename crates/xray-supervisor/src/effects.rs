//! Durable publication evidence for profile and candidate operations.
//! Preparation writes only private immutable artifacts; one atomic file publication is the commit point.
use super::*;

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Publication {
    path: PathBuf,
    before_hash: Option<String>,
    after: Option<String>,
    success: Value,
}
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct EffectJournal {
    transaction_id: String,
    request_digest: String,
    publication: Option<Publication>,
    outcome: Option<Result<Value>>,
}
impl<B: ProxyBackend> Supervisor<B> {
    pub(super) fn effect_path(&self, op: &Operation) -> PathBuf {
        self.journal(&op.transaction_id)
            .with_file_name("effect.json")
    }
    pub(super) fn tracks_effect(command: &Command) -> bool {
        matches!(
            command,
            Command::CreateProfile { .. }
                | Command::DeleteProfile { .. }
                | Command::ValidateDraft { .. }
                | Command::StageHistory { .. }
        )
    }
    fn effect_io<T>(&mut self, result: Result<T>) -> Result<T> {
        result.inspect_err(|e| {
            self.blocked = true;
            self.last_error = Some(e.clone());
        })
    }
    pub(super) fn begin_effect(&mut self, op: &Operation) -> Result<()> {
        let result = write_json(
            &self.effect_path(op),
            &EffectJournal {
                transaction_id: op.transaction_id.clone(),
                request_digest: op.digest.clone(),
                publication: None,
                outcome: None,
            },
        );
        self.effect_io(result)
    }
    fn read_effect(&self, op: &Operation) -> Result<Option<EffectJournal>> {
        let journal = optional_json::<EffectJournal>(&self.effect_path(op)).map_err(|_| {
            Error::new(
                "OPERATION_RECOVERY_REQUIRED",
                "Invalid publication evidence",
            )
        })?;
        if let Some(j) = &journal
            && (j.transaction_id != op.transaction_id || j.request_digest != op.digest)
        {
            return Err(Error::new(
                "OPERATION_RECOVERY_REQUIRED",
                "Publication identity differs from operation",
            ));
        }
        Ok(journal)
    }
    pub(super) fn pending_effect(&self, op: &Operation) -> Result<bool> {
        Ok(self.read_effect(op)?.is_some_and(|j| j.outcome.is_none()))
    }
    /// Optional file hash: absence is an explicit endpoint, other IO failures stay unresolved.
    fn file_hash(path: &Path) -> Result<Option<String>> {
        match fs::read(path) {
            Ok(bytes) => Ok(Some(hash(&bytes))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
    fn resolve_effect(&self, j: &EffectJournal, failure: Option<Error>) -> Result<Result<Value>> {
        if let Some(outcome) = &j.outcome {
            return Ok(outcome.clone());
        }
        let Some(p) = &j.publication else {
            return Ok(Err(failure.unwrap_or_else(|| {
                Error::new(
                    "INTERRUPTED_COMPENSATED",
                    "Preparation interrupted; no public artifact committed",
                )
            })));
        };
        // Evidence paths must remain inside this store even if the journal was edited.
        let allowed = p.path.starts_with(self.store.root.join("config/profiles"))
            || p.path.starts_with(self.store.root.join("candidates"));
        if !allowed
            || p.path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(Error::new(
                "OPERATION_RECOVERY_REQUIRED",
                "Invalid publication target",
            ));
        }
        let current = Self::file_hash(&p.path)?;
        let target = p.after.as_ref().map(|bytes| hash(bytes.as_bytes()));
        if current == target {
            return Ok(Ok(p.success.clone()));
        }
        if current == p.before_hash {
            return Ok(Err(failure.unwrap_or_else(|| {
                Error::new(
                    "INTERRUPTED_COMPENSATED",
                    "Publication did not commit; previous asset preserved",
                )
            })));
        }
        Err(Error::new(
            "OPERATION_RECOVERY_REQUIRED",
            "Publication target changed externally; preserve and repair evidence",
        ))
    }
    pub(super) fn finish_effect(
        &mut self,
        op: &Operation,
        failure: Option<Error>,
    ) -> Result<Option<Result<Value>>> {
        let result = (|| {
            let Some(mut journal) = self.read_effect(op)? else {
                return Ok(None);
            };
            let outcome = self.resolve_effect(&journal, failure)?;
            if journal.outcome.is_none() {
                journal.outcome = Some(outcome.clone());
                write_json(&self.effect_path(op), &journal)?;
            }
            Ok(Some(outcome))
        })();
        self.effect_io(result)
    }
    pub(super) fn publish_effect(
        &mut self,
        op: &Operation,
        path: PathBuf,
        before_hash: Option<String>,
        after: Option<String>,
        success: Value,
    ) -> Result<Value> {
        let result = (|| {
            let mut journal = self.read_effect(op)?.ok_or_else(|| {
                Error::new("OPERATION_RECOVERY_REQUIRED", "Missing preparation intent")
            })?;
            if Self::file_hash(&path)? != before_hash {
                return Err(Error::new(
                    "ASSET_REVISION_CONFLICT",
                    "Artifact changed before publication",
                ));
            }
            journal.publication = Some(Publication {
                path: path.clone(),
                before_hash,
                after: after.clone(),
                success,
            });
            write_json(&self.effect_path(op), &journal)?;
            #[cfg(test)]
            if self.effect_fault == Some(EffectFault::BeforePublish) {
                return Err(Error::new("IO_ERROR", "Injected exit before publication"));
            }
            let effect = if let Some(bytes) = after {
                atomic_write(&path, bytes.as_bytes())
            } else {
                remove_published(&path)
            };
            #[cfg(test)]
            if self.effect_fault == Some(EffectFault::AfterPublish) {
                return Err(Error::new("IO_ERROR", "Injected exit after publication"));
            }
            let outcome = self.resolve_effect(&journal, effect.err())?;
            journal.outcome = Some(outcome.clone());
            write_json(&self.effect_path(op), &journal)?;
            Ok(outcome)
        })();
        self.effect_io(result)?
    }
}
#[cfg(test)]
#[derive(Clone, Copy, PartialEq)]
pub(super) enum EffectFault {
    BeforePublish,
    AfterPublish,
}

fn remove_published(path: &Path) -> Result<()> {
    fs::remove_file(path)?;
    #[cfg(unix)]
    fs::File::open(path.parent().unwrap())?.sync_all()?;
    Ok(())
}
