use super::*;

pub(crate) fn admit_pty_input(svc: &ServiceContext, tab: &str) {
    admit(svc, tab, None);
}

fn admit(svc: &ServiceContext, tab: &str, user_send_id: Option<&str>) {
    let state = svc.ext_or_init::<State>(State::default);
    {
        let mut scheduler = state.scheduler.lock();
        scheduler.input_generation = scheduler.input_generation.saturating_add(1);
        let generation = scheduler.input_generation;
        scheduler.inputs.insert(
            tab.into(),
            InputAdmission {
                generation,
                user_send_id: user_send_id.map(str::to_owned),
            },
        );
    }
    invalidate_boundary(svc, tab);
}

pub(crate) fn admit_pty_write(svc: &ServiceContext, pty: &str, user_send_id: Option<&str>) {
    let tab = {
        let mux = svc.mux.read();
        mux.tab_id_for_pty(pty)
            .and_then(|id| mux.get_tab(&id))
            .filter(|tab| tab.agent_type.is_some())
            .map(|tab| tab.id.clone())
    };
    if let Some(tab) = tab {
        admit(svc, &tab, user_send_id);
    }
}

pub(crate) fn pty_input_witness(
    svc: &ServiceContext,
    tab: &str,
    session: &str,
    nonce: &str,
    row: &codeterm_types::agent::chat_types::RichMessage,
    path: &std::path::Path,
) -> Option<PtyInputWitness> {
    let turn = row
        .provider_observations
        .as_ref()?
        .turn_id
        .as_ref()
        .filter(|turn| !turn.is_empty())?
        .clone();
    let admission = svc
        .try_ext::<State>()?
        .scheduler
        .lock()
        .inputs
        .get(tab)?
        .clone();
    let offset = row.record_start?;
    let epoch = crate::service::tab_chat::resume_cursor::ChatEpoch {
        source_session_id: session.into(),
        session_binding_nonce: None,
    };
    let source = crate::service::tab_chat::resume::capture_incarnation(
        &svc.chat_registry.jsonl_source(tab),
        &epoch,
        path,
        offset,
        None,
    )?;
    let source_row = crate::service::chat_send_transaction::jsonl_source_row(
        session,
        &source.id,
        crate::service::chat_send_transaction::TurnPosition {
            record_start: Some(offset),
            record_row: row.record_row,
        },
    )?;
    let revision = svc.chat_send_ledger.latest_jsonl_input_revision(
        tab,
        nonce,
        admission.user_send_id.as_deref()?,
        &source_row,
    )?;
    let generation = admission.generation;
    let input = PtyInputWitness {
        generation,
        revision,
        turn,
        path: path.into(),
        epoch,
        source,
        offset,
    };
    input.current(svc, tab).then_some(input)
}

impl PtyInputWitness {
    pub(super) fn current(&self, svc: &ServiceContext, tab: &str) -> bool {
        if crate::service::tab_chat::resume::capture_incarnation(
            &svc.chat_registry.jsonl_source(tab),
            &self.epoch,
            &self.path,
            self.offset,
            Some(&self.source),
        )
        .as_ref()
            != Some(&self.source)
            || self.generation == u64::MAX
            || svc.chat_send_ledger.input_revision_for_tab(tab) != self.revision
        {
            return false;
        }
        svc.try_ext::<State>().is_some_and(|state| {
            state
                .scheduler
                .lock()
                .inputs
                .get(tab)
                .is_some_and(|input| input.generation == self.generation)
        })
    }
}

#[cfg(test)]
mod tests;
