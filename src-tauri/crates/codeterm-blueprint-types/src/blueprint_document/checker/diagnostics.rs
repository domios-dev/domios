use super::super::Location;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const CALL_RULE_IDS: &[&str] = &[
    "C1", "C2", "C3", "C4", "C5", "C6", "C7", "C8", "C9", "C10", "C11", "C12",
];

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProofAtom {
    pub id: String,
    pub predicate: String,
    pub arguments: Vec<String>,
    pub present: bool,
    pub field: String,
    pub location: Location,
}

impl ProofAtom {
    pub fn new(
        predicate: &str,
        arguments: Vec<String>,
        present: bool,
        field: &str,
        location: Location,
    ) -> Self {
        let bytes = serde_json::to_vec(&(predicate, &arguments, present, field, &location.file))
            .unwrap_or_default();
        Self {
            id: format!("atom:{:x}", Sha256::digest(bytes)),
            predicate: predicate.into(),
            arguments,
            present,
            field: field.into(),
            location,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Proof {
    pub atom: ProofAtom,
    pub rule: String,
    pub children: Vec<Proof>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Diagnostic {
    pub file: String,
    pub line: usize,
    pub column: usize,
    pub field: String,
    pub severity: String,
    pub message: String,
    pub hint: String,
    pub id: String,
    pub rule: String,
    pub name: String,
    pub failing: Vec<ProofAtom>,
    pub passing: Vec<ProofAtom>,
    pub proof: Proof,
    pub folded: Vec<Diagnostic>,
}

pub struct Finding<'a> {
    pub rule: &'a str,
    pub name: &'a str,
    pub severity: &'a str,
    pub message: &'a str,
    pub hint: &'a str,
}

impl Diagnostic {
    pub fn from_proof(
        finding: Finding<'_>,
        proof: Proof,
        failing: Vec<ProofAtom>,
        passing: Vec<ProofAtom>,
    ) -> Self {
        let Finding {
            rule,
            name,
            severity,
            message,
            hint,
        } = finding;
        let atom = &proof.atom;
        Self {
            file: atom.location.file.clone(),
            line: atom.location.line,
            column: atom.location.column,
            field: atom.field.clone(),
            severity: severity.into(),
            message: message.into(),
            hint: hint.into(),
            id: format!("{rule}:{}", atom.id),
            rule: rule.into(),
            name: name.into(),
            failing,
            passing,
            proof,
            folded: Vec::new(),
        }
    }

    pub fn error(location: Location, field: &str, message: &str, hint: &str) -> Self {
        let atom = ProofAtom::new(
            "invalid_document",
            vec![field.into(), message.into()],
            true,
            field,
            location,
        );
        Self::from_proof(
            Finding {
                rule: "IR",
                name: "invalid_document",
                severity: "error",
                message,
                hint,
            },
            Proof {
                atom: atom.clone(),
                rule: "schema".into(),
                children: Vec::new(),
            },
            vec![atom],
            Vec::new(),
        )
    }
}
