use super::{JsonlSource, SendEvidenceAnchor};
use crate::service::tab_chat::registry::SourceObservation;
use std::{path::PathBuf, sync::Arc};

#[derive(Clone)]
pub(crate) struct FirstMaterialization {
    pub(crate) watch_anchor: SendEvidenceAnchor,
    path: PathBuf,
    witness: Arc<str>,
}

impl FirstMaterialization {
    pub(super) fn new(anchor: SendEvidenceAnchor, path: PathBuf, witness: Arc<str>) -> Self {
        Self {
            watch_anchor: anchor,
            path,
            witness,
        }
    }

    pub(crate) fn resolve(&self, source: &JsonlSource) -> Option<SendEvidenceAnchor> {
        match source.observe(&self.watch_anchor.session_id, &self.path, None, 0) {
            SourceObservation::Present {
                incarnation: Some(incarnation),
                ..
            } if incarnation.absence_witness.as_ref() == Some(&self.witness) => {
                Some(self.watch_anchor.clone())
            }
            _ => None,
        }
    }
}
