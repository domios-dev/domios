use crate::plan::Milestone;

pub const WORKER_STRIDE_LIMIT: usize = 6;

// Pinned decisions lead; named signal kinds (bounce findings, planner notes,
// gate/qa outcomes) next; other kinds fill remaining budget. Pure bookkeeping
const PINNED_KINDS: &[&str] = &["decision"];
const SIGNAL_KINDS: &[&str] = &["bounce", "note", "qa", "regression", "retrospective"];
const NOISE_KINDS: &[&str] = &["status_update"];

fn is_noise(kind: &str) -> bool {
    NOISE_KINDS.contains(&kind)
}

fn tier_rank(kind: &str) -> u8 {
    if PINNED_KINDS.contains(&kind) {
        0
    } else if SIGNAL_KINDS.contains(&kind) {
        1
    } else {
        2
    }
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

// Across kinds, prioritizing pinned/signal; newest-first within a tier, stable
// via id tiebreak. Empty-text and pure-bookkeeping strides are dropped.
pub fn select_worker_strides(milestones: &[Milestone]) -> Vec<&Milestone> {
    let mut selected: Vec<&Milestone> = milestones
        .iter()
        .filter(|m| !is_noise(&m.kind) && !m.text.trim().is_empty())
        .collect();
    selected.sort_by(|a, b| {
        tier_rank(&a.kind)
            .cmp(&tier_rank(&b.kind))
            .then(b.ts.cmp(&a.ts))
            .then(b.id.cmp(&a.id))
    });
    selected.truncate(WORKER_STRIDE_LIMIT);
    selected
}

pub fn render_worker_strides(milestones: &[Milestone]) -> Option<String> {
    let selected = select_worker_strides(milestones);
    if selected.is_empty() {
        return None;
    }
    let mut out = String::from("Recent task strides (read-only context, pinned/newest first):\n");
    for m in selected {
        out.push_str(&format!("- [{}] {}\n", m.kind, one_line(m.text.trim())));
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::AgentRef;

    fn stride(id: &str, kind: &str, text: &str, ts: u64) -> Milestone {
        Milestone {
            id: id.into(),
            from: AgentRef::from_tab("p1"),
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
    fn empty_when_no_relevant_strides() {
        let strides = vec![stride("m1", "status_update", "wip", 10)];
        assert!(render_worker_strides(&strides).is_none());
    }

    #[test]
    fn pinned_decision_leads_then_newest() {
        let strides = vec![
            stride("m1", "note", "old note", 10),
            stride("m2", "decision", "use approach A", 20),
            stride("m3", "bounce", "vague plan", 30),
        ];
        let out = render_worker_strides(&strides).unwrap();
        let decision = out.find("use approach A").unwrap();
        let bounce = out.find("vague plan").unwrap();
        let note = out.find("old note").unwrap();
        assert!(decision < bounce && bounce < note);
        assert!(out.contains("[decision] use approach A"));
    }

    #[test]
    fn respects_cap_and_drops_irrelevant() {
        let mut strides = vec![stride("s", "status_update", "noise", 1)];
        for i in 0..(WORKER_STRIDE_LIMIT + 3) {
            strides.push(stride(
                &format!("m{i}"),
                "note",
                &format!("note {i}"),
                100 + i as u64,
            ));
        }
        let selected = select_worker_strides(&strides);
        assert_eq!(selected.len(), WORKER_STRIDE_LIMIT);
        assert!(selected.iter().all(|m| m.kind != "status_update"));
        // newest note first
        assert_eq!(
            selected[0].text,
            format!("note {}", WORKER_STRIDE_LIMIT + 2)
        );
    }

    #[test]
    fn collapses_multiline_text() {
        let strides = vec![stride("m1", "decision", "line one\n  line two", 10)];
        let out = render_worker_strides(&strides).unwrap();
        assert!(out.contains("line one line two"));
    }
}
