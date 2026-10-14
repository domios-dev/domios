use std::collections::HashMap;
use std::sync::LazyLock;

use super::ServiceContext;

pub(crate) fn should_schedule_owner_clear(kind: Option<codeterm_types::pty::PromptKind>) -> bool {
    kind == Some(codeterm_types::pty::PromptKind::DirectoryTrust)
}

#[derive(Default)]
struct CarrySchedule {
    pending: HashMap<String, PendingCarry>,
}

struct PendingCarry {
    agent_type: String,
    in_flight: bool,
    idle_requested: bool,
    idle_retries: u8,
    output_retries: u8,
    binding_retry_used: bool,
}

/// A declined trust never yields a trusted entry, so idle retries stop after a few turns.
const MAX_IDLE_RETRIES: u8 = 3;
const MAX_OUTPUT_RETRIES: u8 = 3;

impl CarrySchedule {
    fn arm(&mut self, tab_id: &str, agent_type: &str) {
        self.pending
            .entry(tab_id.to_string())
            .or_insert_with(|| PendingCarry {
                agent_type: agent_type.to_string(),
                in_flight: false,
                idle_requested: false,
                idle_retries: 0,
                output_retries: 0,
                binding_retry_used: false,
            });
    }

    fn claim(&mut self, tab_id: &str) -> Option<String> {
        let pending = self.pending.get_mut(tab_id)?;
        if pending.in_flight {
            return None;
        }
        pending.in_flight = true;
        Some(pending.agent_type.clone())
    }

    fn request_idle_retry(&mut self, tab_id: &str) -> bool {
        let Some(pending) = self.pending.get_mut(tab_id) else {
            return false;
        };
        if pending.idle_retries >= MAX_IDLE_RETRIES {
            if pending.output_retries >= MAX_OUTPUT_RETRIES && pending.binding_retry_used {
                self.pending.remove(tab_id);
            }
            return false;
        }
        pending.idle_retries += 1;
        let Some(pending) = self.pending.get_mut(tab_id) else {
            return false;
        };
        if pending.in_flight {
            pending.idle_requested = true;
            false
        } else {
            true
        }
    }

    fn request_binding_retry(&mut self, tab_id: &str) -> bool {
        let Some(pending) = self.pending.get_mut(tab_id) else {
            return false;
        };
        if pending.binding_retry_used {
            return false;
        }
        pending.binding_retry_used = true;
        if pending.in_flight {
            pending.idle_requested = true;
            false
        } else {
            true
        }
    }

    fn request_output_retry(&mut self, tab_id: &str) -> bool {
        let Some(pending) = self.pending.get_mut(tab_id) else {
            return false;
        };
        if pending.output_retries >= MAX_OUTPUT_RETRIES {
            return false;
        }
        pending.output_retries += 1;
        if pending.in_flight {
            pending.idle_requested = true;
            false
        } else {
            true
        }
    }

    fn finish(&mut self, tab_id: &str, success: bool) -> bool {
        if success {
            self.pending.remove(tab_id);
            false
        } else if let Some(pending) = self.pending.get_mut(tab_id) {
            pending.in_flight = false;
            let retry = pending.idle_requested;
            pending.idle_requested = false;
            retry
        } else {
            false
        }
    }
}

static SCHEDULE: LazyLock<parking_lot::Mutex<CarrySchedule>> = LazyLock::new(Default::default);

pub(crate) fn owner_trust_prompt_cleared(service: ServiceContext, tab_id: &str, agent_type: &str) {
    super::directory_trust::invalidate_shared_trust_cache();
    let cwd = service
        .mux
        .read()
        .get_tab(tab_id)
        .and_then(|tab| tab.cwd.clone());
    if let Some(cwd) = cwd {
        service.respond_to_shared_directory_trust_prompts(&cwd, agent_type);
    }
    SCHEDULE.lock().arm(tab_id, agent_type);
    start_attempt(service, tab_id.to_string());
}

pub(crate) fn tab_became_idle(service: ServiceContext, tab_id: &str) {
    if SCHEDULE.lock().request_idle_retry(tab_id) {
        start_attempt(service, tab_id.to_string());
    }
}

pub(crate) fn tab_session_bound(service: ServiceContext, tab_id: &str) {
    let retry = SCHEDULE.lock().request_binding_retry(tab_id);
    if retry {
        start_attempt(service, tab_id.to_string());
    }
}

pub(crate) fn tab_output_changed(service: ServiceContext, tab_id: &str) {
    let retry = SCHEDULE.lock().request_output_retry(tab_id);
    if retry {
        start_attempt(service, tab_id.to_string());
    }
}

pub(crate) fn forget_tab(tab_id: &str) {
    SCHEDULE.lock().pending.remove(tab_id);
}

fn start_attempt(service: ServiceContext, tab_id: String) {
    let Some(agent_type) = SCHEDULE.lock().claim(&tab_id) else {
        return;
    };
    std::thread::spawn(move || {
        let instance_id = crate::paths::codeterm_instance_id().to_string();
        let result = service
            .agent_registry
            .with_provider(&agent_type, |provider| {
                provider.folder_trust_answered(&tab_id, &instance_id)
            });
        let success = match result {
            Some(Ok(carried)) => carried,
            Some(Err(error)) => {
                log::warn!(
                    "[folder-trust] Carry-over failed for Tab {}: {}",
                    tab_id,
                    error
                );
                false
            }
            None => false,
        };
        if success {
            super::directory_trust::invalidate_shared_trust_cache();
            service.respond_to_pending_directory_trust_prompts();
        }
        if SCHEDULE.lock().finish(&tab_id, success) {
            start_attempt(service, tab_id);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::CarrySchedule;
    use super::MAX_OUTPUT_RETRIES;
    use crate::service::folder_trust_carryover::should_schedule_owner_clear;
    use codeterm_types::pty::PromptKind;

    #[test]
    fn owner_answered_trust_prompt_schedules_exactly_one_carry_for_tab() {
        assert!(should_schedule_owner_clear(Some(
            PromptKind::DirectoryTrust
        )));
        let mut schedule = CarrySchedule::default();
        schedule.arm("tab", "codex");
        schedule.arm("tab", "codex");
        assert_eq!(schedule.claim("tab").as_deref(), Some("codex"));
        assert_eq!(schedule.claim("tab"), None);
    }

    #[test]
    fn exit_clear_does_not_schedule_folder_trust_carry() {
        assert!(!should_schedule_owner_clear(None));
        let mut schedule = CarrySchedule::default();
        assert_eq!(schedule.claim("tab"), None);
    }

    #[test]
    fn automatic_directory_trust_responder_does_not_schedule_folder_trust_carry() {
        assert!(!should_schedule_owner_clear(None));
        assert!(!should_schedule_owner_clear(Some(PromptKind::Provider)));
    }

    #[test]
    fn failed_carry_retries_on_a_later_idle_event() {
        let mut schedule = CarrySchedule::default();
        schedule.arm("tab", "codex");
        assert_eq!(schedule.claim("tab").as_deref(), Some("codex"));
        assert!(!schedule.request_idle_retry("tab"));
        assert!(schedule.finish("tab", false));
        assert_eq!(schedule.claim("tab").as_deref(), Some("codex"));
    }

    #[test]
    fn failed_owner_answer_carry_retries_on_session_binding() {
        let mut schedule = CarrySchedule::default();
        schedule.arm("tab", "codex");
        assert_eq!(schedule.claim("tab").as_deref(), Some("codex"));
        assert!(!schedule.finish("tab", false));
        assert!(schedule.request_binding_retry("tab"));
        assert_eq!(schedule.claim("tab").as_deref(), Some("codex"));
        assert!(!schedule.request_binding_retry("tab"));
    }

    #[test]
    fn failed_owner_answer_carry_retries_on_the_next_output_frame() {
        let mut schedule = CarrySchedule::default();
        schedule.arm("tab", "codex");
        assert_eq!(schedule.claim("tab").as_deref(), Some("codex"));
        assert!(!schedule.finish("tab", false));
        for _ in 0..MAX_OUTPUT_RETRIES {
            assert!(schedule.request_output_retry("tab"));
            assert_eq!(schedule.claim("tab").as_deref(), Some("codex"));
            assert!(!schedule.finish("tab", false));
        }
        assert!(!schedule.request_output_retry("tab"));
    }
}
