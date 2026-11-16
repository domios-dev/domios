use super::transport::{Session, SessionState};
use codeterm_types::native_host::{
    NativeHostCancellation, NativeHostCompletion, NativeHostFrame as Frame, NativeHostRejection,
    NativeHostReply, NativeHostRequestId, NativeHostScope, NativeHostUncertainty,
    NativeViewBinding,
};
use serde::{de::DeserializeOwned, Serialize};
use std::sync::atomic::Ordering;
use tokio::sync::oneshot;

impl<UpdateRequest, UpdateResult> Session<UpdateRequest, UpdateResult>
where
    UpdateRequest: Serialize + DeserializeOwned + Send + 'static,
    UpdateResult: Serialize + DeserializeOwned + Send + 'static,
{
    pub(super) fn bind_view(&self, view: NativeViewBinding) {
        if self.retired.load(Ordering::Acquire)
            || !view.is_well_formed()
            || !self.allowed_view_origins.contains(&view.origin)
        {
            return;
        }
        let (replies, cancellations) = {
            let mut state = self.state.lock();
            if self.retired.load(Ordering::Acquire) {
                return;
            }
            let Some(previous) = state.view.replace(view.clone()) else {
                state.diagnostics = None;
                return;
            };
            if previous.session_id == view.session_id {
                if previous.origin == view.origin {
                    return;
                }
                state.view = Some(previous);
                return;
            }
            state.diagnostics = None;
            state.output_attachments.clear();
            let old_session = previous.session_id;
            self.revoke_output_view(old_session);
            let (replies, cancellations) = self.revoke_view_requests(&mut state, old_session);
            (replies, cancellations)
        };
        self.complete_view_revocation(replies, cancellations);
    }

    pub(super) fn release_view(&self, session_id: uuid::Uuid) {
        let (replies, cancellations) = {
            let mut state = self.state.lock();
            if !state
                .view
                .as_ref()
                .is_some_and(|view| view.session_id == session_id)
            {
                return;
            }
            if let Some(view) = &state.view {
                self.revoke_output_view(view.session_id);
            }
            state.view = None;
            state.output_attachments.clear();
            state.diagnostics = None;
            self.revoke_view_requests(&mut state, session_id)
        };
        self.complete_view_revocation(replies, cancellations);
    }

    fn revoke_view_requests(
        &self,
        state: &mut SessionState<UpdateResult>,
        session_id: uuid::Uuid,
    ) -> (
        Vec<(
            oneshot::Sender<NativeHostReply<UpdateResult>>,
            NativeHostReply<UpdateResult>,
        )>,
        Vec<NativeHostRequestId>,
    ) {
        let mut replies = Vec::new();
        let mut cancellations = Vec::new();
        let request_ids = state
            .pending
            .iter()
            .filter_map(|(request_id, call)| {
                (call.scope == NativeHostScope::View(session_id)).then_some(*request_id)
            })
            .collect::<Vec<_>>();
        for request_id in request_ids {
            if state
                .pending
                .get(&request_id)
                .is_some_and(|call| call.dispatched)
            {
                if let Some(call) = state.pending.get_mut(&request_id) {
                    if let Some(reply) = call.reply.take() {
                        let response = NativeHostReply {
                            request_id,
                            connection: self.connection.clone(),
                            scope: call.scope.clone(),
                            completion: NativeHostCompletion::Uncertain(
                                NativeHostUncertainty::CancelledAfterDispatch,
                            ),
                        };
                        replies.push((reply, response));
                    }
                }
                cancellations.push(request_id);
            } else if let Some(mut call) = state.pending.remove(&request_id) {
                if let Some(reply) = call.reply.take() {
                    let response = NativeHostReply {
                        request_id,
                        connection: self.connection.clone(),
                        scope: call.scope,
                        completion: NativeHostCompletion::Rejected(
                            NativeHostRejection::InvalidView,
                        ),
                    };
                    replies.push((reply, response));
                }
            }
        }
        (replies, cancellations)
    }

    fn complete_view_revocation(
        &self,
        replies: Vec<(
            oneshot::Sender<NativeHostReply<UpdateResult>>,
            NativeHostReply<UpdateResult>,
        )>,
        cancellations: Vec<NativeHostRequestId>,
    ) {
        for (sender, reply) in replies {
            let _ = sender.send(reply);
        }
        for request_id in cancellations {
            let cancellation = NativeHostCancellation {
                request_id,
                connection: self.connection.clone(),
            };
            let _ = self.control.try_send(Frame::Cancel(cancellation));
        }
    }
}
