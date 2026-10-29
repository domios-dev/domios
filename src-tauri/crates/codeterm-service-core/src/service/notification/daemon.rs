use super::*;
use crate::ports::daemon_question::DaemonQuestions;
use codeterm_notification_core::contract::{NotificationAction, NotifyLevel};

impl ServiceContext {
    pub fn publish_daemon_question(
        &self,
        request: String,
        title: String,
        body: String,
        choices: Vec<String>,
    ) -> Result<String, String> {
        if request.is_empty()
            || request.len() > 256
            || title.len() > 512
            || body.len() > 4096
            || choices.is_empty()
            || choices.len() > 4
            || choices.iter().enumerate().any(|(index, choice)| {
                choice.is_empty() || choice.len() > 128 || choices[..index].contains(choice)
            })
        {
            return Err("invalid bounded daemon question".into());
        }
        {
            let store = self.notif.lock();
            if let Some(existing) = store.list(false).into_iter().find(|record| {
                store
                    .answer_slot(&record.id)
                    .is_some_and(|slot| slot.daemon_request.as_deref() == Some(&request))
            }) {
                return Ok(existing.id.clone());
            }
        }
        let (id, _) = self.create_notification_actions_reported(
            title,
            Some(body),
            Some("info"),
            Ask {
                daemon_request: Some(request.clone()),
                actions: Some(
                    choices
                        .into_iter()
                        .map(|label| NotificationAction::Answer { label })
                        .collect(),
                ),
                ..Ask::default()
            },
            Some(true),
            NotificationOrigin::default(),
            PushIntent {
                level: Some(NotifyLevel::ActionNeeded),
                event_key: Some(request),
            },
            None,
        );
        Ok(id)
    }

    pub(super) fn answer_host_question(
        &self,
        id: &str,
        label: &str,
    ) -> Option<Result<serde_json::Value, String>> {
        let slot = self.notif.lock().answer_slot(id)?.clone();
        let request = slot.daemon_request?;
        if request.starts_with("mesh-link:") {
            return None;
        }
        Some((|| {
            let handler = self
                .host_extension::<DaemonQuestions>()
                .ok_or("daemon question handler unavailable")?;
            let _claim = handler
                .claim(id)
                .ok_or("daemon question answer is being recorded")?;
            let slot = self
                .notif
                .lock()
                .answer_slot(id)
                .cloned()
                .ok_or("daemon question unavailable")?;
            if slot.delivered {
                return Ok(serde_json::json!({"answered":slot.answered,"delivered":true}));
            }
            let offered = self.notif.lock().notification(id).is_some_and(|record| {
                !record.resolved
                    && record.actions.as_deref().is_some_and(|actions| {
                        actions
                            .iter()
                            .any(|action| action.answer_label() == Some(label))
                    })
            });
            if !offered
                || slot
                    .answered
                    .as_deref()
                    .is_some_and(|standing| standing != label)
            {
                return Err("That choice is no longer available".into());
            }
            let value = handler
                .handler
                .answer(&request, label)
                .ok_or("daemon question type unavailable")??;
            let admission = self.notif.lock().record_answer(id, label);
            match admission {
                AnswerAdmission::Recorded(_) => (),
                AnswerAdmission::AlreadyAnswered(ref standing) if standing == label => (),
                _ => return Err("daemon answer could not be recorded".into()),
            }
            self.notif.lock().mark_answer_delivered(id);
            self.notify_event(
                "notification-answered",
                serde_json::json!({"id":id,"answered":label,"delivered":true}),
            );
            Ok(value)
        })())
    }
}

#[cfg(test)]
mod tests;
