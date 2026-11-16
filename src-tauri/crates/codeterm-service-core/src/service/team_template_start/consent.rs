use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::OnceLock;

use codeterm_team_package::contracts::{declared_asks, ConsentReason, TemplateProvenance};
use codeterm_team_package::Package;
use parking_lot::Mutex;
use sha2::{Digest, Sha256};

const MAX_DECISIONS: usize = 256;

fn identity(package: &Package) -> String {
    let mut hash = Sha256::new();
    for (path, bytes) in package.archive_entries() {
        hash.update((path.len() as u64).to_le_bytes());
        hash.update(path.as_bytes());
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
    }
    format!("{:x}", hash.finalize())
}

#[derive(Debug, Clone)]
struct Issued {
    decision_id: String,
    package: String,
    reasons: Vec<ConsentReason>,
}

#[derive(Default)]
pub struct ConsentLedger {
    issued: Mutex<VecDeque<Issued>>,
    accepted: Mutex<Vec<String>>,
    root: Option<PathBuf>,
}

impl ConsentLedger {
    pub fn persistent(root: PathBuf) -> Self {
        Self {
            root: Some(root),
            ..Self::default()
        }
    }

    pub fn global() -> &'static Self {
        static LEDGER: OnceLock<ConsentLedger> = OnceLock::new();
        LEDGER.get_or_init(|| {
            Self::persistent(crate::paths::data_dir().join("team-template-consent"))
        })
    }

    pub fn issue(&self, package: &Package, reasons: &[ConsentReason]) -> String {
        let decision_id = format!("consent_{}", uuid::Uuid::new_v4().simple());
        let mut issued = self.issued.lock();
        if issued.len() == MAX_DECISIONS {
            issued.pop_front();
        }
        issued.push_back(Issued {
            decision_id: decision_id.clone(),
            package: identity(package),
            reasons: reasons.to_vec(),
        });
        decision_id
    }

    pub fn admits(
        &self,
        decision_id: Option<&str>,
        package: &Package,
        reasons: &[ConsentReason],
    ) -> bool {
        let Some(decision_id) = decision_id.map(str::trim).filter(|id| !id.is_empty()) else {
            return false;
        };
        let identity = identity(package);
        self.issued.lock().iter().any(|issued| {
            issued.decision_id == decision_id
                && issued.package == identity
                && reasons.iter().all(|reason| issued.reasons.contains(reason))
        })
    }

    pub fn accepted(&self, package: &Package) -> bool {
        let identity = identity(package);
        match &self.root {
            Some(root) => {
                std::fs::read(root.join(&identity)).is_ok_and(|bytes| bytes == b"accepted")
            }
            None => self.accepted.lock().contains(&identity),
        }
    }

    pub fn accept(&self, package: &Package) -> Result<(), String> {
        let identity = identity(package);
        match &self.root {
            Some(root) => {
                std::fs::create_dir_all(root).map_err(|error| error.to_string())?;
                codeterm_util::safe_tree::refuse_linked_ancestors(root)?;
                codeterm_util::atomic_write::write_atomic(&root.join(identity), b"accepted")
                    .map_err(|error| error.to_string())
            }
            None => {
                let mut accepted = self.accepted.lock();
                if !accepted.contains(&identity) {
                    accepted.push(identity);
                }
                Ok(())
            }
        }
    }
}

pub fn consent_reasons(package: &Package, provenance: TemplateProvenance) -> Vec<ConsentReason> {
    if !matches!(
        provenance,
        TemplateProvenance::Imported | TemplateProvenance::Channel
    ) {
        return Vec::new();
    }
    let mut reasons = vec![ConsentReason::UnverifiedPackage];
    if !declared_asks(package).is_empty() {
        reasons.push(ConsentReason::NewGrants);
    }
    reasons
}

#[cfg(test)]
mod tests;
