//! The funnel every tab lifecycle commit goes through. Callers journal what
//! they just committed; nothing here infers a transition from state.

use crate::session::lifecycle::{LifecycleKey, LifecycleTransition, TabLifecycleJournal};

use super::ServiceContext;

impl ServiceContext {
    /// Open a lifecycle operation and bind the tab to it. Later transitions
    /// key off the returned lineage.
    pub(crate) fn open_lifecycle_epoch(&self, tab_id: &str) -> Option<LifecycleKey> {
        // Only the PTY transport commits lifecycle transitions, so only it may
        // carry lineage. A chatBackend tab stays pre-journal by construction.
        if self
            .mux
            .read()
            .get_tab(tab_id)
            .is_some_and(|tab| tab.is_chat_backend())
        {
            return None;
        }
        let journal = TabLifecycleJournal::shared();
        let epoch = crate::session::lifecycle::next_epoch_for(&journal.load(), tab_id);
        let key = LifecycleKey::new(tab_id, epoch, uuid::Uuid::new_v4().to_string());
        if !self
            .mux
            .write()
            .stamp_lifecycle(tab_id, key.epoch, &key.operation_id)
        {
            return None;
        }
        append(journal, key.clone(), LifecycleTransition::EpochOpened);
        Some(key)
    }

    /// Journal a transition against the tab's current operation. No open
    /// operation records nothing, so nothing unearned becomes authority.
    pub(crate) fn record_lifecycle(&self, tab_id: &str, transition: LifecycleTransition) {
        let open_operation = self.mux.read().lifecycle_key(tab_id);
        let Some(key) = open_operation else {
            log::debug!(
                "[tab-lifecycle] {} has no open operation; {} not journaled",
                tab_id,
                transition.label()
            );
            return;
        };
        append(TabLifecycleJournal::shared(), key, transition);
    }

    /// End the tab's authority and drop its lineage, so no snapshot can match
    /// a dead operation.
    pub(crate) fn record_lifecycle_end(&self, tab_id: &str, transition: LifecycleTransition) {
        self.record_lifecycle(tab_id, transition);
        if let Some(tab) = self.mux.write().get_tab_mut(tab_id) {
            tab.lifecycle_epoch = None;
            tab.lifecycle_operation_id = None;
        }
    }

    /// Journal the role/Team authority this operation just committed.
    pub(crate) fn record_role_transition(&self, tab_id: &str) {
        let committed = {
            let mux = self.mux.read();
            mux.get_tab(tab_id).map(|tab| {
                (
                    tab.role.map(|role| format!("{role:?}")),
                    tab.role_profile.clone(),
                    tab.can_spawn,
                    tab.depth,
                    tab.team_id.clone(),
                    tab.parent_tab_id.clone(),
                )
            })
        };
        let Some((role, role_profile, can_spawn, depth, team_id, parent_tab_id)) = committed else {
            return;
        };
        self.record_lifecycle(
            tab_id,
            LifecycleTransition::RoleBinding {
                role,
                role_profile,
                can_spawn,
                depth,
            },
        );
        self.record_lifecycle(
            tab_id,
            LifecycleTransition::TeamBinding {
                team_id,
                parent_tab_id,
            },
        );
    }
}

fn append(journal: &TabLifecycleJournal, key: LifecycleKey, transition: LifecycleTransition) {
    let label = transition.label();
    if let Err(error) = journal.append(key.clone(), transition) {
        log::warn!(
            "[tab-lifecycle] {} epoch {} {} not journaled: {error}",
            key.tab_id,
            key.epoch,
            label
        );
    }
}
