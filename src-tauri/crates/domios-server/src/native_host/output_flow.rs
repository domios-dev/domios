use super::transport::{NativeHostEndpoint, Session};
use codeterm_types::native_host::NativeOutputFlow;
use serde::{de::DeserializeOwned, Serialize};
use std::sync::atomic::Ordering;
use std::time::Instant;
use uuid::Uuid;

const MAX_PRIMARY_OUTPUT_ATTACHMENTS: usize = 64;

impl<Request, Reply> NativeHostEndpoint<Request, Reply> {
    pub fn install_output_service(&self, service: codeterm_service_core::service::ServiceState) {
        *self.output_service.write() = Some(std::sync::Arc::downgrade(&service));
    }
}

impl<Request, Reply> Session<Request, Reply>
where
    Request: Serialize + DeserializeOwned + Send + 'static,
    Reply: Serialize + DeserializeOwned + Send + 'static,
{
    pub(super) fn revoke_output_view(&self, view: Uuid) {
        if let Some(service) = self
            .output_service
            .read()
            .as_ref()
            .and_then(std::sync::Weak::upgrade)
        {
            service.screen_state.revoke_output_view(view);
        }
    }

    pub(super) fn accept_output_flow(&self, view: Uuid, update: NativeOutputFlow) {
        let service = self
            .output_service
            .read()
            .as_ref()
            .and_then(std::sync::Weak::upgrade);
        let Some(service) = service else {
            return;
        };
        let mux = service.mux.read();
        let mut state = self.state.lock();
        if self.retired.load(Ordering::Acquire)
            || self.active_generation.load(Ordering::Acquire) != self.connection.generation
            || !state
                .view
                .as_ref()
                .is_some_and(|binding| binding.session_id == view)
        {
            return;
        }
        match update {
            NativeOutputFlow::DetachAll => {
                state.output_attachments.clear();
                service.screen_state.revoke_output_view(view);
            }
            NativeOutputFlow::Detach { pty_id, attachment } => {
                if state
                    .output_attachments
                    .get(&pty_id)
                    .is_some_and(|(owner, _)| *owner == attachment)
                {
                    state.output_attachments.remove(&pty_id);
                }
                if let Some(flow) = service.screen_state.output_flow(&pty_id) {
                    flow.detach(view, Some(attachment));
                }
            }
            NativeOutputFlow::Rendered {
                tab_id,
                pty_id,
                attachment,
                offset,
                attach,
            } => {
                if cfg!(windows) {
                    return;
                }
                let Some(tab) = mux.get_tab(&tab_id) else {
                    return;
                };
                if tab.pty_id.as_deref() != Some(pty_id.as_str())
                    || !codeterm_service_core::pty::output_flow::renderer_may_pause(
                        tab.agent_booting,
                        tab.agent_type.is_some(),
                        tab.boot_stage,
                    )
                {
                    return;
                }
                let Some(flow) = service.screen_state.output_flow(&pty_id) else {
                    return;
                };
                let Some(current_generation) = service.screen_state.pty_generation(&pty_id) else {
                    return;
                };
                if attach {
                    if state.output_attachments.len() >= MAX_PRIMARY_OUTPUT_ATTACHMENTS
                        && !state.output_attachments.contains_key(&pty_id)
                    {
                        return;
                    }
                    if state
                        .output_attachments
                        .get(&pty_id)
                        .is_none_or(|(owner, generation)| {
                            *owner != attachment || *generation != current_generation
                        })
                    {
                        flow.detach(view, None);
                        state
                            .output_attachments
                            .insert(pty_id.clone(), (attachment, current_generation));
                    }
                }
                let Some(&(owner, generation)) = state.output_attachments.get(&pty_id) else {
                    return;
                };
                if owner != attachment || generation != current_generation {
                    return;
                }
                let now = Instant::now();
                if attach || !flow.acknowledge(view, attachment, offset, now) {
                    flow.attach(generation, view, attachment, offset, now);
                }
            }
        }
    }
}
