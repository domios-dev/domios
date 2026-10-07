use super::*;

impl UserSendStore {
    pub(crate) fn claim_rpc_group(
        &self,
        records: &[UserSendRecord],
        session: &str,
    ) -> Result<bool, String> {
        let Some(first) = records.first() else {
            return Ok(false);
        };
        let tab = self.tab(&first.authority.tab_id);
        let mut data = tab.data.lock();
        let loaded = self.load(&tab, &mut data)?;
        if records.iter().any(|record| {
            record.authority != first.authority
                || !self.eligible(record)
                || record
                    .authority
                    .session_id
                    .as_deref()
                    .is_some_and(|expected| expected != session)
                || !loaded.index.records.iter().any(|current| {
                    current.send_id == record.send_id
                        && current.state == UserSendState::Queued
                        && current.authority == record.authority
                        && current.owner_epoch == record.owner_epoch
                })
        }) {
            return Ok(false);
        }
        let mut next = loaded.index.clone();
        for record in records {
            let current = next
                .records
                .iter_mut()
                .find(|current| current.send_id == record.send_id)
                .ok_or("group member disappeared")?;
            current.state = UserSendState::Claimed;
            current.handoff_session = Some(session.into());
        }
        self.persist(&tab, &next)?;
        loaded.index = next;
        Ok(true)
    }
}
