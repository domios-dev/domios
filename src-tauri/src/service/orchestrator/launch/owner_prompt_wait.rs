use crate::agent::provider::{ComposerCursorPosition, ProviderReadinessStrategy};
use crate::service::launch_input_gate::{LaunchInputEvidence, LaunchInputRefusal};
use crate::service::launch_readiness::LaunchAttempt;
use crate::service::ServiceContext;
use std::time::{Duration, Instant};

pub(super) fn await_input_readiness(
    service: &ServiceContext,
    tab_id: &str,
    pty_id: &str,
    agent_id: &str,
    attempt: &LaunchAttempt,
    session_starts_on_first_turn: bool,
    startup_binding_deadline: Duration,
    input_readiness_timeout: Duration,
    provider_readiness: ProviderReadinessStrategy,
    composer_cursor_position: ComposerCursorPosition,
) -> Result<LaunchInputEvidence, LaunchInputRefusal> {
    let mut launch_input_deadline = Instant::now() + startup_binding_deadline;
    let mut resumed_after_owner_prompt = false;
    loop {
        if crate::service::launch_readiness::owner_prompt_is_active(tab_id) {
            if let Some(question) =
                crate::service::launch_readiness::await_owner_prompt_resolution(tab_id)
            {
                return Err(LaunchInputRefusal::OwnerPromptDeadline(question));
            }
            resumed_after_owner_prompt = true;
            launch_input_deadline = Instant::now() + startup_binding_deadline;
            continue;
        }
        let readiness = if session_starts_on_first_turn {
            if !resumed_after_owner_prompt {
                let provider_frame_wait =
                    if composer_cursor_position == ComposerCursorPosition::VisibleAnywhere {
                        ProviderReadinessStrategy::TuiFrame {
                            timeout: input_readiness_timeout,
                        }
                    } else {
                        provider_readiness.clone()
                    };
                let _ = service.await_provider_readiness_on_screen(
                    pty_id,
                    agent_id,
                    provider_frame_wait,
                );
            }
            service.await_lazy_session_input_readiness(
                attempt,
                agent_id,
                launch_input_deadline.saturating_duration_since(Instant::now()),
            )
        } else {
            service.await_launch_input_readiness(attempt, agent_id, input_readiness_timeout)
        };
        if readiness.is_err() {
            if crate::service::launch_readiness::owner_prompt_is_active(tab_id) {
                continue;
            }
            let stage_waited_for_owner = service.mux.read().get_tab(tab_id).is_some_and(|tab| {
                tab.boot_stage == Some(crate::service::boot_stage::BootStage::AwaitingOwnerPrompt)
            });
            if stage_waited_for_owner && !resumed_after_owner_prompt {
                resumed_after_owner_prompt = true;
                launch_input_deadline = Instant::now() + startup_binding_deadline;
                continue;
            }
        }
        return readiness;
    }
}
