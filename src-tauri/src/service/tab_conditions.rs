use codeterm_types::notification::NotificationAction;

use super::hierarchy::{resolve_report_target, ReportTarget};
use super::message_gateway::{DeliveryClass, MessageBody, MessageOrigin, OutboundMessage};
use super::messenger::MessageEnvelope;
use super::ServiceContext;
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{Tab, TabAudience};
use crate::notification::dedupe::{ConditionTransition, TabCondition};

pub(crate) fn is_user_facing(tab: &Tab) -> bool {
    !tab.hidden && tab.audience == TabAudience::User
}

/// Who a tab's standing conditions are owed to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ConditionOwner {
    User,
    /// Not user-facing: the live local tab its reports go to, if there is one.
    Reporter(Option<String>),
}

pub(crate) fn condition_owner(mux: &MultiplexerState, tab_id: &str) -> Option<ConditionOwner> {
    let tab = mux.get_tab(tab_id)?;
    if is_user_facing(tab) {
        return Some(ConditionOwner::User);
    }
    if tab.audience == TabAudience::UserOwnedAutomation {
        let creator = tab
            .spawned_by_tab_id
            .as_deref()
            .filter(|creator| *creator != tab_id && mux.get_tab(creator).is_some());
        return Some(match creator {
            Some(creator) => ConditionOwner::Reporter(Some(creator.to_string())),
            None => ConditionOwner::User,
        });
    }
    let target = match resolve_report_target(mux, tab_id) {
        Ok((_, Some(ReportTarget::Local(target)))) if mux.get_tab(&target).is_some() => {
            Some(target)
        }
        _ => None,
    };
    Some(ConditionOwner::Reporter(target))
}

/// What one condition says, to whichever owner it reaches. `hint` is the extra
/// line an agent owner needs to act on it without the user's controls.
pub(crate) struct ConditionNotice {
    pub title: String,
    pub body: String,
    pub actions: Option<Vec<NotificationAction>>,
    pub hint: Option<String>,
}

/// Where one raised condition went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ConditionRoute {
    /// A user-facing record, or the live one this condition already holds.
    Notified(String),
    /// Reported to the owning agent tab instead of the user.
    Reported {
        to: String,
    },
    /// Already reported and not yet resolved: the same news stays quiet.
    AlreadyReported,
    /// Not user-facing and no agent owns it: recorded in the log only.
    Unowned,
    TabGone,
}

impl ServiceContext {
    pub(crate) fn tab_is_user_facing(&self, tab_id: &str) -> bool {
        self.mux.read().get_tab(tab_id).is_some_and(is_user_facing)
    }

    pub(crate) fn raise_tab_condition(
        &self,
        tab_id: &str,
        condition: TabCondition,
        notice: ConditionNotice,
    ) -> ConditionRoute {
        let (owner, label) = {
            let mux = self.mux.read();
            (
                condition_owner(&mux, tab_id),
                mux.display_label_for_tab(tab_id),
            )
        };
        if condition == TabCondition::PendingPrompt {
            self.set_cron_owner_prompt_waiting(tab_id, true);
        }
        let target = match owner {
            None => return ConditionRoute::TabGone,
            Some(ConditionOwner::User) => {
                return ConditionRoute::Notified(self.create_condition_notification(
                    tab_id,
                    condition,
                    notice.title,
                    Some(notice.body),
                    notice.actions,
                ));
            }
            Some(ConditionOwner::Reporter(target)) => target,
        };
        if self.notif.lock().observe_tab_condition(tab_id, condition)
            == ConditionTransition::AlreadyAnnounced
        {
            return ConditionRoute::AlreadyReported;
        }
        let Some(target) = target else {
            log::info!(
                "[tab-condition] tab={tab_id} condition={} is not user-facing and has no report target; kept off the user's surfaces",
                condition.as_str()
            );
            return ConditionRoute::Unowned;
        };
        let mut body = format!("{}: {}", notice.title, notice.body);
        if let Some(hint) = notice.hint {
            body.push('\n');
            body.push_str(&hint);
        }
        let message = OutboundMessage {
            target: target.clone(),
            body: MessageBody::Text { body },
            envelope: Some(MessageEnvelope::report(label, tab_id)),
            class: DeliveryClass::SystemNotice,
            origin: MessageOrigin::System,
            idempotency_key: None,
        };
        // The owner may be mid-turn; its delivery must not hold the output path that observed the condition.
        let svc = self.clone();
        let reporter = tab_id.to_string();
        let owner = target.clone();
        std::thread::spawn(move || match svc.gateway.submit(&svc, message) {
            Ok(_) => log::info!(
                "[tab-condition] tab={reporter} condition={} reported to {owner}",
                condition.as_str()
            ),
            Err(error) => log::warn!(
                "[tab-condition] tab={reporter} condition={} report failed: {error}",
                condition.as_str()
            ),
        });
        ConditionRoute::Reported { to: target }
    }

    /// A fresh agent process reached readiness: the previous process's exit is
    /// over, and so is any prompt it was blocked on unless one is showing now.
    pub(crate) fn retire_superseded_process_conditions(&self, tab_id: &str) {
        let prompt_showing = self
            .mux
            .read()
            .get_tab(tab_id)
            .is_some_and(|tab| tab.pending_prompt.is_some());
        let resolved = {
            let mut store = self.notif.lock();
            let mut resolved = store.resolve_tab_condition(tab_id, TabCondition::AgentExited);
            if !prompt_showing {
                resolved.extend(store.resolve_tab_condition(tab_id, TabCondition::PendingPrompt));
            }
            resolved
        };
        self.announce_resolved(&resolved);
    }
}

#[cfg(test)]
#[path = "tab_conditions_tests.rs"]
mod tests;
