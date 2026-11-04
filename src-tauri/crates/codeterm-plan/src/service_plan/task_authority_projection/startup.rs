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

pub(in crate::service_plan) struct StartupSubject<'a> {
    pub id: &'a str,
    pub executor: &'a str,
    pub session: Option<&'a str>,
    pub group: Option<&'a str>,
}

pub(in crate::service_plan) fn startup_subject_current(
    tab: &str,
    assigned_tab: Option<&str>,
    first: StartupSubject<'_>,
    latest_unclosed: Option<&str>,
    latest_group: Option<&str>,
) -> bool {
    if first.executor != tab {
        return false;
    }
    match first.group {
        Some(group) => {
            first
                .session
                .is_some_and(|session| !session.trim().is_empty())
                && latest_group == Some(group)
        }
        None => assigned_tab == Some(tab) && latest_unclosed == Some(first.id),
    }
}

impl TaskStartupRead {
    fn current_first(&self, tab: &str) -> Option<&StartupAttemptRead> {
        let first = self.first_matching.as_ref()?;
        startup_subject_current(
            tab,
            self.assigned_tab.as_deref(),
            StartupSubject {
                id: &first.id,
                executor: &first.executor,
                session: first.session.as_deref(),
                group: first.group.as_deref(),
            },
            self.latest_unclosed
                .as_ref()
                .map(|latest| latest.id.as_str()),
            self.latest_group
                .as_ref()
                .map(|latest| latest.group.as_str()),
        )
        .then_some(first)
    }

    pub fn owns(&self, tab: &str, session: Option<&str>, clear_report: bool) -> bool {
        matches!(self.status, TaskStatus::Pending | TaskStatus::InProgress)
            && self.current_first(tab).is_some_and(|first| {
                first.acknowledged_at.is_none()
                    && (!clear_report || (first.reported_at.is_none() && !first.has_report))
                    && session.is_none_or(|session| first.session.as_deref() == Some(session))
            })
    }

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
        self.current_first(tab)?;
        Some(DispatchStartupOwner {
            attempt_id: latest.id.clone(),
            tab_id: tab.into(),
        })
    }
}
