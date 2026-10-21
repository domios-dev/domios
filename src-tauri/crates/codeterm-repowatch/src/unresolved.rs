//! A path that resolved to no repository stays refused until its shape
//! changes, so repeated registrations neither re-probe git nor re-report.

use crate::events::RepoWatchError;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PathShape {
    pub exists: bool,
    pub has_git: bool,
}

impl PathShape {
    pub(crate) fn probe(path: &Path) -> Self {
        Self {
            exists: path.exists(),
            has_git: path.join(".git").exists(),
        }
    }
}

#[derive(Default)]
pub(crate) struct UnresolvedPaths {
    refused: HashMap<String, (PathShape, RepoWatchError)>,
}

impl UnresolvedPaths {
    pub(crate) fn refusal(&self, key: &str, shape: PathShape) -> Option<RepoWatchError> {
        self.refused
            .get(key)
            .filter(|(refused_shape, _)| *refused_shape == shape)
            .map(|(_, error)| error.clone())
    }

    pub(crate) fn refuse(&mut self, key: String, shape: PathShape, error: RepoWatchError) {
        self.refused.insert(key, (shape, error));
    }

    pub(crate) fn resolved(&mut self, key: &str) {
        self.refused.remove(key);
    }
}
