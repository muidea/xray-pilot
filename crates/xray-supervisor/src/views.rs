//! Bounded operation views; complete evidence stays in the operation store.
use super::*;

impl Operation {
    pub(super) fn view(&self, blocked: bool) -> Result<Value> {
        let bytes = serde_json::to_vec(self)?;
        let recovery_required = self.recovery_required || (blocked && self.response.is_none());
        if bytes.len() <= 1024 * 1024 {
            let mut value = serde_json::to_value(self)?;
            value["recovery_required"] = json!(recovery_required);
            return Ok(value);
        }
        let (state, error) = match &self.response {
            Some(Response { result: Ok(_), .. }) => ("Succeeded", None),
            Some(Response {
                result: Err(error), ..
            }) => ("Failed", Some(error)),
            None => (
                if recovery_required {
                    "RecoveryRequired"
                } else {
                    "Pending"
                },
                None,
            ),
        };
        // A large error message must not make the summary itself unbounded.
        let error = error.map(|e| {
            json!({"code":e.code.chars().take(160).collect::<String>(),
            "message":e.message.chars().take(2048).collect::<String>()})
        });
        let command = serde_json::to_value(&self.request.command)?["operation"].clone();
        Ok(
            json!({"view":"Summary", "request_id":self.request.request_id,
            "command":command, "digest":self.digest, "transaction_id":self.transaction_id,
            "state":state,"error":error,"recovery_required":recovery_required,
            "details":{"sha256":hash(&bytes),"total_bytes":bytes.len(),"max_chunk_bytes":1048576}}),
        )
    }
    pub(super) fn details(
        &self,
        expected_hash: &str,
        offset: usize,
        limit: usize,
    ) -> Result<Value> {
        if !(1..=1048576).contains(&limit) {
            return Err(Error::new(
                "INVALID_PAGE",
                "Operation detail limit must be 1–1048576 bytes",
            ));
        }
        let bytes = serde_json::to_vec(self)?;
        let digest = hash(&bytes);
        if digest != expected_hash {
            return Err(Error::new(
                "OPERATION_DETAILS_CHANGED",
                "Operation changed; refresh its summary before reading details",
            ));
        }
        if offset > bytes.len() {
            return Err(Error::new(
                "INVALID_PAGE",
                "Operation detail offset exceeds its size",
            ));
        }
        let end = offset.saturating_add(limit).min(bytes.len());
        Ok(
            json!({"sha256":digest,"total_bytes":bytes.len(),"offset":offset,
            "bytes":bytes[offset..end],"next_offset":(end < bytes.len()).then_some(end)}),
        )
    }
}
