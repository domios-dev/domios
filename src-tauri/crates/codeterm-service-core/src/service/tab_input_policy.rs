use super::ServiceContext;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabInputError {
    CuratorReadOnly,
}

impl std::fmt::Display for TabInputError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CuratorReadOnly => f.write_str("curator_read_only: Curator round, read-only"),
        }
    }
}

fn authorize(
    curator: bool,
    source: Option<&str>,
    owner: Option<&str>,
) -> Result<(), TabInputError> {
    if curator && (source.is_none() || source != owner) {
        return Err(TabInputError::CuratorReadOnly);
    }
    Ok(())
}

impl ServiceContext {
    pub(crate) fn require_admitted_input_allowed(
        &self,
        target: &str,
        send_id: Option<&str>,
    ) -> Result<(), String> {
        let routed = send_id
            .and_then(|id| self.chat_send_ledger.status(id))
            .is_some_and(|status| status.tab_id == target && status.system_send.is_some());
        if routed {
            return Ok(());
        }
        self.require_tab_input_allowed(target, None)
    }

    pub fn require_tab_input_allowed(
        &self,
        target: &str,
        source: Option<&str>,
    ) -> Result<(), String> {
        let (id, curator, source) = {
            let mux = self.mux.read();
            let id = mux
                .resolve_tab_id(target)
                .or_else(|| mux.tab_id_for_pty(target));
            if let Some(refusal) = id.as_deref().and_then(|id| mux.tab_write_lock(id)) {
                return Err(refusal.to_string());
            }
            let curator = id
                .as_deref()
                .and_then(|id| mux.get_tab(id))
                .is_some_and(|tab| tab.role_profile.as_deref() == Some("curator"));
            let source = source.and_then(|id| mux.resolve_tab_id(id));
            (id, curator, source)
        };
        let owner = if curator {
            id.and_then(|id| {
                self.try_ext::<super::curator::CuratorTabs>()
                    .and_then(|index| index.0.read().get(&id).cloned())
            })
            .and_then(|plan| self.plan_store.owner_of(&plan))
            .and_then(|owner| owner.tab_id)
        } else {
            None
        };
        authorize(curator, source.as_deref(), owner.as_deref()).map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests;
