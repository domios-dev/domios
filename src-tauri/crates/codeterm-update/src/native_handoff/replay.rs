use super::NativeUpdateHandoff;
use codeterm_types::native_update::{
    apply::{NativeUpdateApplyOptions, NativeUpdateApplyResult},
    NativeUpdateOperationId, NativeUpdateRequest,
};
use serde::{Deserialize, Serialize};

pub const MAX_RECENT_RESULTS: usize = 16;
const FILTER_WORDS: usize = 1024;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayLedger {
    pub(super) recent: Vec<RecordedAttempt>,
    // Bits never clear: evicted IDs remain refused, and collisions only refuse fresh admission.
    used: Vec<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct RecordedAttempt {
    pub request: NativeUpdateRequest,
    pub options: NativeUpdateApplyOptions,
    pub result: NativeUpdateApplyResult,
}

impl ReplayLedger {
    pub(super) fn contains(&self, id: NativeUpdateOperationId) -> bool {
        if self.used.is_empty() {
            return false;
        }
        if self.used.len() != FILTER_WORDS || self.recent.len() > MAX_RECENT_RESULTS {
            return true;
        }
        positions(id)
            .into_iter()
            .all(|bit| self.used[bit / 64] & (1 << (bit % 64)) != 0)
    }

    pub(super) fn after(previous: &NativeUpdateHandoff, id: NativeUpdateOperationId) -> Self {
        let mut ledger = previous.replay.clone();
        if ledger.used.is_empty() {
            ledger.used.resize(FILTER_WORDS, 0);
        }
        ledger.remember(previous.request.operation_id);
        ledger.remember(id);
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
        for bit in positions(id) {
            self.used[bit / 64] |= 1 << (bit % 64);
        }
    }
}

fn positions(id: NativeUpdateOperationId) -> [usize; 4] {
    use sha2::Digest;
    let bytes = sha2::Sha256::digest(id.value().into_bytes());
    std::array::from_fn(|index| {
        let offset = index * 4;
        u32::from_le_bytes([
            bytes[offset],
            bytes[offset + 1],
            bytes[offset + 2],
            bytes[offset + 3],
        ]) as usize
            % (FILTER_WORDS * 64)
    })
}
