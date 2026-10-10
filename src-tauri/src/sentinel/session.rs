use super::store::Sentinel;

#[derive(Default)]
pub(super) struct Usage {
    pub exchanges: usize,
    pub actions: usize,
    pub recent: usize,
}

pub(super) fn usage(jobs: &[Sentinel], source: &Sentinel, now: u64) -> Usage {
    let root = source.scenario_id.as_ref().unwrap_or(&source.id);
    let mut usage = Usage::default();
    for job in jobs
        .iter()
        .filter(|job| job.same_owner(source) && job.scenario_id.as_ref().unwrap_or(&job.id) == root)
    {
        usage.exchanges += job.transitions.len();
        for transition in &job.transitions {
            usage.recent += usize::from(now.saturating_sub(transition.accepted_at) < 60_000);
            usage.actions += transition
                .handlers
                .iter()
                .filter(|result| result.claim_id.is_some())
                .count();
        }
    }
    usage
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blueprint_session_safety_counts_share_graph_and_exclude_foreign_scope() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = crate::sentinel::tests::store(
            dir.path(),
            crate::cron::clock::MockClock::starting_at(1_000),
        );
        let mut root = crate::sentinel::tests::create(&mut store, "team");
        root.transitions
            .push(crate::sentinel::handlers::Transition {
                causal_run: None,
                decision: None,
                id: "root-event".into(),
                bucket: root.buckets[0].clone(),
                accepted_at: 0,
                terminal: false,
                handlers: Vec::new(),
            });
        let mut child = root.clone();
        child.id = "child".into();
        child.scenario_id = Some(root.id.clone());
        child.transitions[0].accepted_at = 1;
        let mut foreign = child.clone();
        foreign.team_id = Some("foreign".into());
        let usage = usage(&[root.clone(), child, foreign], &root, 60_000);
        assert_eq!(usage.exchanges, 2);
        assert_eq!(usage.recent, 1);
        assert_eq!(usage.actions, 0);
    }
}
