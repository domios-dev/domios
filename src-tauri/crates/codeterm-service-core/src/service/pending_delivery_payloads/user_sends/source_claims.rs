use super::*;
use crate::service::chat_send_transaction::SendRowClaim;

pub(super) fn validate_claims(claims: &[SendRowClaim], tab: &str) -> Result<(), String> {
    let mut sends = HashSet::new();
    let mut rows = HashSet::new();
    if claims.len() > continuity::MAX_RECEIPTS
        || claims.iter().any(|claim| {
            claim.tab_id != tab
                || uuid::Uuid::parse_str(&claim.send_id).is_err()
                || !claim.matched_row.source.valid()
                || claim.matched_row.ordinal == 0
                || !sends.insert(&claim.send_id)
                || !rows.insert(&claim.matched_row)
        })
    {
        return Err("user source row claims are invalid".into());
    }
    Ok(())
}

impl UserSendStore {
    pub(crate) fn source_claims(&self, tab_id: &str) -> Result<Vec<SendRowClaim>, String> {
        let tab = self.tab(tab_id);
        let mut data = tab.data.lock();
        Ok(self.load(&tab, &mut data)?.index.source_claims.clone())
    }

    pub(crate) fn record_source_claims(
        &self,
        tab_id: &str,
        claims: &[SendRowClaim],
    ) -> Result<(), String> {
        if claims.is_empty() {
            return Ok(());
        }
        let tab = self.tab(tab_id);
        let mut data = tab.data.lock();
        let loaded = self.load(&tab, &mut data)?;
        let mut next = loaded.index.clone();
        for claim in claims {
            if next.source_claims.iter().any(|prior| {
                prior.send_id != claim.send_id && prior.matched_row == claim.matched_row
            }) {
                return Err("user source row ordinal already claimed".into());
            }
            if let Some(prior) = next
                .source_claims
                .iter()
                .find(|prior| prior.send_id == claim.send_id)
            {
                if prior != claim {
                    return Err("user source row claim changed".into());
                }
            } else {
                next.source_claims.push(claim.clone());
            }
        }
        if next.source_claims == loaded.index.source_claims {
            return Ok(());
        }
        if next.source_claims.len() > continuity::MAX_RECEIPTS {
            next.source_claims
                .drain(..next.source_claims.len() - continuity::MAX_RECEIPTS);
        }
        validate_claims(&next.source_claims, tab_id)?;
        self.persist(&tab, &next)?;
        loaded.index = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
