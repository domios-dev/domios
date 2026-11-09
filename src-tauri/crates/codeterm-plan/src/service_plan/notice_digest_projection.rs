#[derive(Debug, Clone, serde::Serialize)]
pub struct NoticeDigestInput {
    pub plan_id: String,
    pub team_id: Option<String>,
    pub owner_tab_id: Option<String>,
    pub project_root: String,
    pub queue: NoticeQueueRead,
    pub cartography: NoticeCartographyRead,
    pub finished_bindings: Vec<NoticeFinishedBinding>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct NoticeQueueRead {
    pub in_review: usize,
    pub in_progress: usize,
    pub oldest_in_review_age_ms: u64,
    pub stalled_lanes: usize,
    pub finished_lanes: usize,
    pub lane_candidates: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct NoticeCartographyRead {
    pub latest_map_at: u64,
    pub release_at: Option<u64>,
    pub closed_group: Option<NoticeClosedGroup>,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct NoticeClosedGroup {
    pub key: String,
    pub label: String,
    pub closed_at: u64,
}

#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct NoticeInventoryTargets {
    pub task_ids: Vec<String>,
    pub tab_ids: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct NoticeFinishedBinding {
    pub task_id: String,
    pub assigned_tab_id: Option<String>,
}

impl NoticeDigestInput {
    pub fn queue_counters(&self) -> crate::service_plan::digest_rules::PlanDigestCounters {
        crate::service_plan::digest_rules::PlanDigestCounters {
            plan_id: self.plan_id.clone(),
            in_review: self.queue.in_review,
            in_progress: self.queue.in_progress,
            oldest_in_review_age_ms: self.queue.oldest_in_review_age_ms,
            stalled_lanes: self.queue.stalled_lanes,
            finished_lanes: self.queue.finished_lanes,
            lane_candidates: self.queue.lane_candidates.clone(),
            jobs: 0,
            agent_tabs: 0,
            idle_utility_shells: 0,
            idle_workers: Vec::new(),
        }
    }
}
