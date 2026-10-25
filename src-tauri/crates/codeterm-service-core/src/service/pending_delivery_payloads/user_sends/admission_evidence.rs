use super::*;

#[derive(Serialize)]
pub struct UserAdmissionEvidence {
    #[serde(rename = "correlationId")]
    client_token: String,
    evidence: &'static str,
}

fn admission_evidence(present: bool, history_complete: bool, owner_failed: bool) -> &'static str {
    if present {
        "present"
    } else if history_complete && !owner_failed {
        "absent"
    } else {
        "unknown"
    }
}

impl UserSendStore {
    pub fn non_durable_admission_evidence(
        tokens: &[String],
        ledger_tokens: &[String],
    ) -> Result<Vec<UserAdmissionEvidence>, String> {
        if tokens.len() > 64 || tokens.iter().any(|token| token.len() > 4096) {
            return Err("admission evidence query exceeds metadata bound".into());
        }
        Ok(tokens
            .iter()
            .map(|token| UserAdmissionEvidence {
                client_token: token.clone(),
                evidence: if ledger_tokens.contains(token) {
                    "present"
                } else {
                    "not_applicable"
                },
            })
            .collect())
    }

    pub fn admission_evidence(
        &self,
        tab_id: &str,
        tokens: &[String],
        ledger_tokens: &[String],
    ) -> Result<Vec<UserAdmissionEvidence>, String> {
        if tokens.len() > 64 || tokens.iter().any(|token| token.len() > 4096) {
            return Err("admission evidence query exceeds metadata bound".into());
        }
        let tab = self.tab(tab_id);
        let mut data = tab.data.lock();
        let index = &self.load(&tab, &mut data)?.index;
        let admissions = tab.admissions.lock();
        // An evicted receipt cannot prove absence of an earlier admission.
        let history_complete = index.next_sequence <= continuity::MAX_RECEIPTS as u64 + 1;
        let owner_failed = tab.failed.load(Ordering::Acquire);
        Ok(tokens
            .iter()
            .map(|token| {
                let present = ledger_tokens.contains(token)
                    || admissions.tokens.existing(token).is_some()
                    || index
                        .records
                        .iter()
                        .any(|record| record.client_token.as_ref() == Some(token))
                    || index
                        .receipts
                        .iter()
                        .any(|receipt| receipt.client_token.as_ref() == Some(token));
                UserAdmissionEvidence {
                    client_token: token.clone(),
                    evidence: admission_evidence(present, history_complete, owner_failed),
                }
            })
            .collect())
    }
}

#[cfg(test)]
mod tests;
