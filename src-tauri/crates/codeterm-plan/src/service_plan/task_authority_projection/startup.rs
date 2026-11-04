use crate::plan::TaskStatus;
use crate::service_plan::DispatchStartupOwner;

#[derive(serde::Deserialize)]
pub(in crate::service_plan) struct TaskStartupRead {
    pub status: TaskStatus,
    pub assigned_tab: Option<String>,
    pub latest_open: Option<StartupAttemptRead>,
    pub first_matching: Option<StartupAttemptRead>,
    pub latest_unclosed: Option<AttemptIdRead>,
    pub latest_group: Option<CompetitionGroupRead>,
}

#[derive(serde::Deserialize)]
pub(in crate::service_plan) struct StartupAttemptRead {
    pub id: String,
    pub executor: String,
    pub title: Option<String>,
    pub session: Option<String>,
    pub group: Option<String>,
    pub acknowledged_at: Option<u64>,
    pub reported_at: Option<u64>,
    pub has_report: bool,
}

#[derive(serde::Deserialize)]
pub(in crate::service_plan) struct AttemptIdRead {
    pub id: String,
}

#[derive(serde::Deserialize)]
pub(in crate::service_plan) struct CompetitionGroupRead {
    pub group: String,
}

impl TaskStartupRead {
    pub fn owner(&self) -> Option<DispatchStartupOwner> {
        let tab = self.assigned_tab.as_deref()?;
        let latest = self.latest_open.as_ref()?;
        if latest.executor != tab
            || latest.title.is_none()
            || latest.acknowledged_at.is_some()
            || latest.reported_at.is_some()
            || latest.has_report
            || latest.group.is_some()
            || !matches!(self.status, TaskStatus::Pending | TaskStatus::InProgress)
        {
            return None;
        }
        let first = self.first_matching.as_ref()?;
        let valid = if let Some(group) = first.group.as_deref() {
            first
                .session
                .as_deref()
                .is_some_and(|session| !session.trim().is_empty())
                && self
                    .latest_group
                    .as_ref()
                    .is_some_and(|current| current.group == group)
        } else {
            first.executor == tab
                && self
                    .latest_unclosed
                    .as_ref()
                    .is_some_and(|current| current.id == first.id)
        };
        valid.then(|| DispatchStartupOwner {
            attempt_id: latest.id.clone(),
            tab_id: tab.into(),
        })
    }
}
