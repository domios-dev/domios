use crate::plan::{Milestone, Plan};

pub const RETROSPECTIVE_KIND: &str = "retrospective";
const SUPERSEDE_NOTE: &str = "supersedes previous retrospective";

pub fn latest_retrospective(plan: &Plan) -> Option<&Milestone> {
    plan.strides
        .iter()
        .filter(|m| m.kind == RETROSPECTIVE_KIND && m.text.trim() != SUPERSEDE_NOTE)
        .max_by_key(|m| m.ts)
}

pub fn has_retrospective(plan: &Plan) -> bool {
    latest_retrospective(plan).is_some()
}

pub fn retrospective_supersede_note() -> &'static str {
    SUPERSEDE_NOTE
}

pub fn skill_timeline_entries<'a>(plan: &'a Plan, slug: &str) -> Vec<&'a Milestone> {
    let needle = format!("skill {slug}");
    plan.strides
        .iter()
        .filter(|m| m.kind == "skill" && m.text.starts_with(&needle))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{AgentRef, OrchestratorRef, Plan};

    fn stride(kind: &str, text: &str, ts: u64) -> Milestone {
        Milestone {
            id: format!("ms_{ts}"),
            from: AgentRef::from_pane("p1"),
            kind: kind.into(),
            text: text.into(),
            ts,
            links: vec![],
            loop_id: None,
            iteration: None,
            canvas: Default::default(),
            attachments: vec![],
        }
    }

    #[test]
    fn latest_retrospective_skips_supersede_markers() {
        let mut plan = Plan::new("P".into(), "/tmp".into(), OrchestratorRef::from_pane("o"));
        plan.strides = vec![
            stride(RETROSPECTIVE_KIND, "first draft", 100),
            stride(RETROSPECTIVE_KIND, SUPERSEDE_NOTE, 200),
            stride(RETROSPECTIVE_KIND, "final write-up", 300),
        ];
        let latest = latest_retrospective(&plan).unwrap();
        assert_eq!(latest.text, "final write-up");
    }

    #[test]
    fn has_retrospective_ignores_supersede_only_history() {
        let mut plan = Plan::new("P".into(), "/tmp".into(), OrchestratorRef::from_pane("o"));
        plan.strides = vec![stride(RETROSPECTIVE_KIND, SUPERSEDE_NOTE, 100)];
        assert!(!has_retrospective(&plan));
    }
}
