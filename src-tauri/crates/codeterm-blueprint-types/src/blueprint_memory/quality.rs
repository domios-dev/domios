#[derive(Clone, Debug, Default)]
pub struct Evidence {
    pub tags: Vec<String>,
    pub curator: Option<CuratorEvidence>,
    pub origin: Option<String>,
    pub kept: bool,
    pub successes: u64,
    pub failures: u64,
    pub unclassified: u64,
    pub recognized: u64,
    pub runs: u64,
    pub name: String,
    pub note: String,
    pub created_at_ms: u64,
    pub creator_deleted_at_ms: Option<u64>,
    pub stopped_before_success: bool,
    pub safe: bool,
}

#[derive(Clone, Debug)]
pub struct CuratorEvidence {
    pub plan: String,
    pub round: u64,
    pub episodes: Vec<String>,
    pub granted: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Qualification {
    pub save: bool,
    pub reason: &'static str,
}

pub fn decide(evidence: &Evidence) -> Qualification {
    let reason = if !evidence.safe {
        "unsafe_template"
    } else if evidence.kept {
        "explicit_keep"
    } else if evidence.successes == 0 {
        "no_success"
    } else if evidence.unclassified.saturating_add(evidence.failures) > evidence.recognized {
        "mostly_unrecognized_or_failed"
    } else if evidence.stopped_before_success {
        "stopped_before_success"
    } else if evidence
        .creator_deleted_at_ms
        .is_some_and(|at| at.saturating_sub(evidence.created_at_ms) < 600_000)
    {
        "deleted_as_mistake"
    } else if evidence.note.trim().is_empty() {
        if evidence.runs <= 1
            && evidence
                .name
                .to_lowercase()
                .split(|c: char| !c.is_alphanumeric())
                .any(|word| matches!(word, "test" | "probe" | "tmp" | "temp"))
        {
            "throwaway"
        } else {
            "missing_note"
        }
    } else {
        "proven_solution"
    };
    Qualification {
        save: matches!(reason, "explicit_keep" | "proven_solution"),
        reason,
    }
}
