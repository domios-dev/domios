use std::collections::BTreeMap;
use std::sync::LazyLock;
use tauri::Manager;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use super::ServiceContext;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LaunchOrigin {
    Ui,
    #[default]
    Unknown,
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct LaunchRecord {
    origin: LaunchOrigin,
    identity: String,
    restored: bool,
}

static LAUNCHES: LazyLock<Mutex<Option<BTreeMap<String, LaunchRecord>>>> = LazyLock::new(|| {
    let path = launch_path();
    let state = match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).ok(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Some(BTreeMap::new()),
        Err(_) => None,
    };
    Mutex::new(state)
});

fn launch_path() -> std::path::PathBuf {
    codeterm_paths::data_dir()
        .join("supporter")
        .join("launch-origins.json")
}

pub(crate) fn record_ui_origin(tab_id: &str) {
    update_launch(tab_id, true, false);
}

pub fn record_launch(tab_id: &str, restored: bool) {
    update_launch(tab_id, false, restored);
}

pub(crate) fn launch_was_restored(tab_id: &str) -> Option<bool> {
    LAUNCHES
        .lock()
        .as_ref()
        .and_then(|records| records.get(tab_id))
        .map(|record| record.restored)
}

pub(crate) fn spawn_ok_notice_allowed(recipient_was_restored: Option<bool>) -> bool {
    recipient_was_restored == Some(false)
}

fn update_launch(tab_id: &str, ui: bool, restored: bool) {
    let mut all = LAUNCHES.lock();
    let Some(records) = all.as_mut() else {
        return;
    };
    if !records.contains_key(tab_id) && records.len() >= 4096 {
        return;
    }
    let record = records.entry(tab_id.into()).or_default();
    if ui {
        record.origin = LaunchOrigin::Ui;
    }
    if !restored || record.identity.is_empty() {
        record.identity = uuid::Uuid::new_v4().to_string();
    }
    record.restored = restored;
    let result = serde_json::to_vec(records)
        .map_err(std::io::Error::other)
        .and_then(|bytes| codeterm_util::atomic_write::write_atomic(&launch_path(), bytes));
    if let Err(error) = result {
        log::error!("[supporter-notify] Cannot persist launch identity: {error}");
        *all = None;
    }
}

#[derive(Default)]
pub(crate) struct LaunchFacts {
    pub(crate) origin: LaunchOrigin,
    pub(crate) parent_or_spawner: bool,
    pub(crate) team_owned: bool,
    pub(crate) cron: bool,
    pub(crate) hidden: bool,
    pub(crate) general_agent: bool,
}

pub(crate) fn suppress_owner_notification(label: &str, facts: &LaunchFacts) -> bool {
    matches!(
        label,
        "trust_or_permission_prompt" | "login_or_auth_required"
    ) && facts.origin == LaunchOrigin::Ui
        && !facts.parent_or_spawner
        && !facts.team_owned
        && !facts.cron
        && !facts.hidden
        && !facts.general_agent
}

#[derive(Clone)]
pub(crate) struct Notice {
    pub(crate) tab: String,
    pub(crate) title: String,
    pub(crate) label: String,
    pub(crate) launch: String,
    pub(crate) body: String,
}

#[derive(Default)]
pub(crate) struct RestoreQueue {
    complete: bool,
    wave_settled: bool,
    observations: usize,
    pending: BTreeMap<(String, String, String), Notice>,
}

impl RestoreQueue {
    pub(crate) fn accept(&mut self, notice: Notice, restored: bool) -> Vec<Notice> {
        if restored && !self.complete {
            self.pending.insert(
                (
                    notice.tab.clone(),
                    notice.label.clone(),
                    notice.launch.clone(),
                ),
                notice,
            );
            Vec::new()
        } else {
            vec![notice]
        }
    }

    pub(crate) fn begin_observation(&mut self) -> bool {
        if self.complete {
            return false;
        }
        self.observations += 1;
        true
    }

    pub(crate) fn finish_observation(&mut self) -> BTreeMap<String, Vec<Notice>> {
        self.observations = self.observations.saturating_sub(1);
        self.flush_settled()
    }

    pub(crate) fn complete(&mut self) -> BTreeMap<String, Vec<Notice>> {
        self.wave_settled = true;
        self.flush_settled()
    }

    fn flush_settled(&mut self) -> BTreeMap<String, Vec<Notice>> {
        if !self.wave_settled || self.observations != 0 {
            return BTreeMap::new();
        }
        self.complete = true;
        let mut grouped = BTreeMap::<String, Vec<Notice>>::new();
        for notice in std::mem::take(&mut self.pending).into_values() {
            grouped
                .entry(notice.label.clone())
                .or_default()
                .push(notice);
        }
        grouped
    }
}

static RESTORE: LazyLock<Mutex<RestoreQueue>> = LazyLock::new(Default::default);

pub(crate) struct RestoreObservation {
    service: ServiceContext,
}

impl Drop for RestoreObservation {
    fn drop(&mut self) {
        let groups = RESTORE.lock().finish_observation();
        for notices in groups.into_values() {
            emit_batch(&self.service, notices);
        }
    }
}

pub(crate) fn begin_restore_observation(
    service: &ServiceContext,
    tab_id: &str,
) -> Option<RestoreObservation> {
    let restored = LAUNCHES
        .lock()
        .as_ref()
        .and_then(|records| records.get(tab_id))
        .is_some_and(|record| record.restored);
    if !restored || !RESTORE.lock().begin_observation() {
        return None;
    }
    Some(RestoreObservation {
        service: service.clone(),
    })
}

pub(crate) fn owner(service: &ServiceContext, tab_id: &str, label: &str, body: &str) {
    let tab = { service.mux.read().get_tab(tab_id).cloned() };
    let Some(tab) = tab else {
        return;
    };
    let record = LAUNCHES
        .lock()
        .as_ref()
        .and_then(|all| all.get(tab_id))
        .cloned()
        .unwrap_or_default();
    let facts = LaunchFacts {
        origin: record.origin,
        parent_or_spawner: tab.parent_tab_id.is_some()
            || tab.spawned_by_tab_id.is_some()
            || tab.task_binding.is_some(),
        team_owned: tab.team_id.is_some(),
        cron: service
            .app_handle
            .try_state::<crate::CronState>()
            .is_some_and(|cron| cron.lock().get_tab_owner(tab_id).is_some()),
        hidden: tab.hidden,
        general_agent: super::general_agent::is_general_agent(&tab),
    };
    if suppress_owner_notification(label, &facts) {
        return;
    }
    // Unknown launch provenance cannot claim a durable notification identity.
    if record.identity.is_empty() {
        return;
    }
    let notice = Notice {
        tab: tab_id.into(),
        title: tab.title,
        label: label.into(),
        launch: record.identity,
        body: body.into(),
    };
    let ready = RESTORE
        .lock()
        .accept(notice, record.restored || tab.restored_from_snapshot);
    for notice in ready {
        emit_batch(service, vec![notice]);
    }
}

pub(crate) fn restore_complete(service: &ServiceContext) {
    let groups = RESTORE.lock().complete();
    for notices in groups.into_values() {
        emit_batch(service, notices);
    }
}

fn emit_batch(service: &ServiceContext, notices: Vec<Notice>) {
    let eligible: Vec<_> = notices
        .into_iter()
        .filter(|notice| {
            let key = super::supporter::criteria_hash(
                format!("{}\0{}\0{}", notice.tab, notice.label, notice.launch).as_bytes(),
            );
            super::supporter::runtime()
                .claim_notification(&key)
                .unwrap_or(false)
        })
        .collect();
    let Some(first) = eligible.first() else {
        return;
    };
    let title = if eligible.len() == 1 {
        format!("{} needs attention", first.title)
    } else {
        format!("{} tabs wait on {}", eligible.len(), first.label)
    };
    let body = eligible
        .iter()
        .map(|n| format!("{}: {}", n.title, n.body))
        .collect::<Vec<_>>()
        .join("\n");
    service.log_system_notice(title, Some(body), Some("warning"));
}

pub(crate) fn verbose(service: &ServiceContext, tab: &str, title: &str, body: &str) {
    service.log_system_notice(
        title.into(),
        Some(format!("Tab {tab}: {body}")),
        Some("info"),
    );
}

pub(crate) fn failure(service: &ServiceContext, tab: &str, title: &str, body: &str) {
    service.log_system_notice(
        title.into(),
        Some(format!("Tab {tab}: {body}")),
        Some("error"),
    );
}

#[cfg(test)]
mod tests;
