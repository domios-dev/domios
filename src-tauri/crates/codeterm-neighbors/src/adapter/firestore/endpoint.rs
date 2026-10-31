use crate::{Error, Result};

const GOOGLE: &str = "https://firestore.googleapis.com";
const EMULATOR: &str = "http://127.0.0.1:18080";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FirestoreTarget {
    project: String,
    local: bool,
}

impl FirestoreTarget {
    pub fn google(project: &str) -> Result<Self> {
        Self::resolve(false, false, GOOGLE, project)
    }

    pub fn debug_emulator(target: &str, project: &str) -> Result<Self> {
        Self::resolve(cfg!(debug_assertions), true, target, project)
    }

    fn resolve(debug: bool, selected: bool, target: &str, project: &str) -> Result<Self> {
        let local = debug && selected && target == EMULATOR && project == "demo-domios";
        let cloud =
            !selected && target == GOOGLE && matches!(project, "domios-d9357" | "domios-signin-qa");
        if !local && !cloud {
            return Err(Error::Transport);
        }
        Ok(Self {
            project: project.into(),
            local,
        })
    }

    pub fn project(&self) -> &str {
        &self.project
    }

    pub(super) fn uri(&self) -> &'static str {
        if self.local {
            EMULATOR
        } else {
            GOOGLE
        }
    }

    pub(super) fn tls(&self) -> bool {
        !self.local
    }

    pub(super) fn database(&self) -> String {
        format!("projects/{}/databases/(default)", self.project)
    }
}

#[cfg(test)]
mod tests;
