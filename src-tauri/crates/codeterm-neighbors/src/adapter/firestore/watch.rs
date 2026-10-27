use super::{fs, Error, Result};
use std::collections::HashSet;

#[derive(Default)]
pub(super) struct Membership {
    documents: HashSet<String>,
    observe_only: bool,
}

impl Membership {
    pub(super) fn new(observe_only: bool) -> Self {
        Self {
            observe_only,
            ..Default::default()
        }
    }
    pub(super) fn observe(&mut self, response: &fs::ListenResponse) -> Result<bool> {
        use fs::listen_response::ResponseType;
        match response.response_type.as_ref() {
            Some(ResponseType::DocumentChange(change)) => {
                let document = change.document.as_ref().ok_or(Error::ListenProtocol {
                    kind: "document_missing",
                })?;
                if !self.observe_only && change.target_ids.contains(&1) {
                    self.documents.insert(document.name.clone());
                }
                if change.removed_target_ids.contains(&1) {
                    self.documents.remove(&document.name);
                }
            }
            Some(ResponseType::DocumentDelete(change)) => {
                self.documents.remove(&change.document);
            }
            Some(ResponseType::DocumentRemove(change)) => {
                if change.removed_target_ids.contains(&1) {
                    self.documents.remove(&change.document);
                }
            }
            Some(ResponseType::Filter(filter)) if filter.target_id == 1 && !self.observe_only => {
                if usize::try_from(filter.count).ok() != Some(self.documents.len()) {
                    return Err(Error::ListenProtocol {
                        kind: "existence_filter_mismatch",
                    });
                }
            }
            Some(ResponseType::TargetChange(change))
                if change.target_change_type
                    == fs::target_change::TargetChangeType::Reset as i32
                    && (change.target_ids.is_empty() || change.target_ids.contains(&1)) =>
            {
                self.documents.clear();
                return Ok(true);
            }
            None => {
                return Err(Error::ListenProtocol {
                    kind: "response_missing",
                })
            }
            _ => {}
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests;
