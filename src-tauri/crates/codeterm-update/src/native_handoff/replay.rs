use super::NativeUpdateHandoff;
use codeterm_types::native_update::{
    apply::{NativeUpdateApplyOptions, NativeUpdateApplyResult},
    NativeUpdateOperationId, NativeUpdateRequest,
};
use serde::{Deserialize, Serialize};

pub const MAX_RECENT_RESULTS: usize = 16;
pub const MAX_INCARNATION_ADMISSIONS: usize = 256;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayLedger {
    pub(super) recent: Vec<RecordedAttempt>,
    used: Vec<NativeUpdateOperationId>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct RecordedAttempt {
    pub request: NativeUpdateRequest,
    pub options: NativeUpdateApplyOptions,
    pub result: NativeUpdateApplyResult,
}

pub(super) fn same_incarnation(left: &NativeUpdateRequest, right: &NativeUpdateRequest) -> bool {
    left.expected_server.identity == right.expected_server.identity
        && left.expected_server.incarnation.epoch == right.expected_server.incarnation.epoch
        && left.expected_server.incarnation.process == right.expected_server.incarnation.process
}

impl ReplayLedger {
    pub(super) fn new(id: NativeUpdateOperationId) -> Self {
        Self {
            recent: Vec::new(),
            used: vec![id],
        }
    }

    pub(super) fn contains(&self, id: NativeUpdateOperationId) -> bool {
        self.used.contains(&id)
    }

    pub(super) fn at_capacity(&self, current: NativeUpdateOperationId) -> bool {
        self.used.len() + usize::from(!self.contains(current)) >= MAX_INCARNATION_ADMISSIONS
    }

    pub(super) fn after(previous: &NativeUpdateHandoff, request: &NativeUpdateRequest) -> Self {
        let mut ledger = previous.replay.clone();
        if same_incarnation(&previous.request, request) {
            ledger.remember(previous.request.operation_id);
        } else {
            ledger.used.clear();
        }
        ledger.remember(request.operation_id);
        ledger.recent.push(RecordedAttempt {
            request: previous.request.clone(),
            options: previous.options.clone(),
            result: previous.result.clone(),
        });
        let excess = ledger.recent.len().saturating_sub(MAX_RECENT_RESULTS);
        ledger.recent.drain(..excess);
        ledger
    }

    fn remember(&mut self, id: NativeUpdateOperationId) {
        if !self.contains(id) {
            self.used.push(id);
        }
    }
}
