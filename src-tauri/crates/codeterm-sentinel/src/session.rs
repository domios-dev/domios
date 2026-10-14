use super::store::Sentinel;

#[derive(Default)]
pub struct Usage {
    pub exchanges: usize,
    pub actions: usize,
    pub recent: usize,
}

pub fn usage(jobs: &[Sentinel], source: &Sentinel, now: u64) -> Usage {
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
