use std::collections::VecDeque;
use std::sync::OnceLock;

use codeterm_team_package::contracts::{declared_asks, ConsentReason, TemplateProvenance};
use codeterm_team_package::Package;
use codeterm_types::team::role_registration::PinnedPackage;
use parking_lot::Mutex;

/// Bounds the decisions kept between preview and start; the oldest is forgotten first.
const MAX_DECISIONS: usize = 256;

#[derive(Debug, Clone)]
struct Issued {
    decision_id: String,
    package: PinnedPackage,
    reasons: Vec<ConsentReason>,
}

/// Owner decisions the preview contract issued. A start needing consent must carry one
/// recorded for the same package identity and covering every current reason.
pub struct ConsentLedger {
    issued: Mutex<VecDeque<Issued>>,
}

impl Default for ConsentLedger {
    fn default() -> Self {
        Self {
            issued: Mutex::new(VecDeque::new()),
        }
    }
}

impl ConsentLedger {
    pub fn global() -> &'static Self {
        static LEDGER: OnceLock<ConsentLedger> = OnceLock::new();
        LEDGER.get_or_init(Self::default)
    }

    pub fn issue(&self, package: &PinnedPackage, reasons: &[ConsentReason]) -> String {
        let decision_id = format!("consent_{}", uuid::Uuid::new_v4().simple());
        let mut issued = self.issued.lock();
        if issued.len() == MAX_DECISIONS {
            issued.pop_front();
        }
        issued.push_back(Issued {
            decision_id: decision_id.clone(),
            package: package.clone(),
            reasons: reasons.to_vec(),
        });
        decision_id
    }

    pub fn admits(
        &self,
        decision_id: Option<&str>,
        package: &PinnedPackage,
        reasons: &[ConsentReason],
    ) -> bool {
        let Some(decision_id) = decision_id.map(str::trim).filter(|id| !id.is_empty()) else {
            return false;
        };
        self.issued.lock().iter().any(|issued| {
            issued.decision_id == decision_id
                && issued.package == *package
                && reasons.iter().all(|reason| issued.reasons.contains(reason))
        })
    }
}

/// An imported package is unverified; any declared ask is a grant the owner has not given yet.
pub fn consent_reasons(package: &Package, provenance: TemplateProvenance) -> Vec<ConsentReason> {
    let mut reasons = Vec::new();
    if provenance == TemplateProvenance::Imported {
        reasons.push(ConsentReason::UnverifiedPackage);
    }
    if !declared_asks(package).is_empty() {
        reasons.push(ConsentReason::NewGrants);
    }
    reasons
}
