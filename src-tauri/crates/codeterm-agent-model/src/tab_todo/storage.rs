use codeterm_storage::{
    TabTodoAmendmentRecord, TabTodoChecklistRecord, TabTodoItemEventRecord, TabTodoItemRecord,
    TabTodoItemRefRecord, TabTodoNewItemRecord, TabTodoOwnWriteBase, TabTodoOwnWriteRecord,
    TabTodoRepository, TabTodoStateChangeRecord,
};
use codeterm_types::agent::{
    TabTodoChecklist, TabTodoChecklistStatus, TabTodoCounters, TabTodoEventKind, TabTodoItem,
    TabTodoItemEvent, TabTodoLink, TabTodoNewItem, TabTodoState, TabTodoWriteBase,
    TabTodoWritePlan, TabTodoWriteTarget,
};

/// Typed domain boundary over the shared Storage tab-todo repository.
#[derive(Clone)]
pub struct TabTodoStore {
    repository: TabTodoRepository,
}

impl TabTodoStore {
    pub fn new(repository: TabTodoRepository) -> Self {
        Self { repository }
    }

    pub async fn get(&self, tab_id: String) -> Result<Option<TabTodoChecklist>, String> {
        self.repository
            .get(tab_id)
            .await
            .map(|read| read.value.map(checklist).transpose())
            .map_err(storage_error)?
    }

    pub async fn create(
        &self,
        tab_id: String,
        workspace_id: String,
        team_id: Option<String>,
        started_at: u64,
        items: Vec<TabTodoNewItem>,
        truncated_item_ids: Vec<String>,
    ) -> Result<TabTodoChecklist, String> {
        let records = new_items(items)?;
        checklist(
            self.repository
                .create(
                    tab_id,
                    workspace_id,
                    team_id,
                    started_at,
                    records,
                    truncated_item_ids,
                )
                .await
                .map_err(storage_error)?
                .value,
        )
    }

    pub async fn append(
        &self,
        tab_id: String,
        generation: u64,
        actor: Option<String>,
        at: u64,
        items: Vec<TabTodoNewItem>,
    ) -> Result<TabTodoChecklist, String> {
        checklist(
            self.repository
                .append(tab_id, generation, actor, at, new_items(items)?)
                .await
                .map_err(storage_error)?
                .value,
        )
    }

    pub async fn set_state(
        &self,
        tab_id: String,
        generation: u64,
        item_id: String,
        expected_revision: u64,
        state: TabTodoState,
        reason: Option<String>,
        actor: Option<String>,
        at: u64,
    ) -> Result<TabTodoChecklist, String> {
        checklist(
            self.repository
                .set_state(
                    tab_id,
                    generation,
                    item_id,
                    expected_revision,
                    state_string(state).into(),
                    reason,
                    actor,
                    at,
                )
                .await
                .map_err(storage_error)?
                .value,
        )
    }

    pub async fn reopen(
        &self,
        tab_id: String,
        generation: u64,
        item_id: String,
        reason: String,
        actor: Option<String>,
        at: u64,
    ) -> Result<TabTodoChecklist, String> {
        checklist(
            self.repository
                .reopen(tab_id, generation, item_id, reason, actor, at)
                .await
                .map_err(storage_error)?
                .value,
        )
    }

    pub async fn transfer(
        &self,
        source_tab_id: String,
        target_tab_id: String,
        workspace_id: String,
        team_id: Option<String>,
        actor: Option<String>,
        at: u64,
    ) -> Result<Option<TabTodoChecklist>, String> {
        self.repository
            .transfer(
                source_tab_id,
                target_tab_id,
                workspace_id,
                team_id,
                actor,
                at,
            )
            .await
            .map_err(storage_error)?
            .value
            .map(checklist)
            .transpose()
    }

    pub async fn amend(
        &self,
        tab_id: String,
        generation: u64,
        item_id: String,
        expected_revision: u64,
        amendment: TabTodoNewItem,
        actor: Option<String>,
        at: u64,
    ) -> Result<TabTodoChecklist, String> {
        let amendment = TabTodoAmendmentRecord {
            title: amendment.title,
            description: amendment.description,
            link_json: encode_link(amendment.link)?,
        };
        checklist(
            self.repository
                .amend(
                    tab_id,
                    generation,
                    item_id,
                    expected_revision,
                    amendment,
                    actor,
                    at,
                )
                .await
                .map_err(storage_error)?
                .value,
        )
    }

    pub async fn clear_one(
        &self,
        tab_id: String,
        generation: u64,
        item_id: String,
        expected_revision: u64,
        actor: Option<String>,
        at: u64,
    ) -> Result<TabTodoChecklist, String> {
        checklist(
            self.repository
                .clear_one(tab_id, generation, item_id, expected_revision, actor, at)
                .await
                .map_err(storage_error)?
                .value,
        )
    }

    pub async fn clear_all(
        &self,
        tab_id: String,
        generation: u64,
        actor: Option<String>,
        at: u64,
    ) -> Result<TabTodoChecklist, String> {
        checklist(
            self.repository
                .clear_all(tab_id, generation, actor, at)
                .await
                .map_err(storage_error)?
                .value,
        )
    }

    pub async fn reset(
        &self,
        tab_id: String,
        generation: u64,
        workspace_id: String,
        team_id: Option<String>,
        started_at: u64,
        actor: Option<String>,
        items: Vec<TabTodoNewItem>,
    ) -> Result<TabTodoChecklist, String> {
        checklist(
            self.repository
                .reset(
                    tab_id,
                    generation,
                    workspace_id,
                    team_id,
                    started_at,
                    actor,
                    new_items(items)?,
                )
                .await
                .map_err(storage_error)?
                .value,
        )
    }

    /// Applies an owner's planned write; `appended` resolves `plan.appends` in order.
    pub async fn write_own(
        &self,
        tab_id: String,
        workspace_id: String,
        team_id: Option<String>,
        at: u64,
        generation: u64,
        plan: &TabTodoWritePlan,
        appended: Vec<TabTodoNewItem>,
    ) -> Result<TabTodoChecklist, String> {
        let state_changes = plan
            .state_changes
            .iter()
            .map(|change| {
                let (item_id, expected_revision) = match &change.target {
                    TabTodoWriteTarget::Existing(item) => {
                        (item.item_id.clone(), item.expected_revision)
                    }
                    TabTodoWriteTarget::Appended(index) => appended
                        .get(*index)
                        .map(|item| (item.id.clone(), 1))
                        .ok_or_else(|| {
                            "todo write state change names no appended item".to_string()
                        })?,
                };
                Ok(TabTodoStateChangeRecord {
                    item_id,
                    expected_revision,
                    state: state_string(change.state).into(),
                    reason: change.reason.clone(),
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let write = TabTodoOwnWriteRecord {
            base: match plan.base {
                TabTodoWriteBase::Fresh => TabTodoOwnWriteBase::Fresh,
                TabTodoWriteBase::Clear => TabTodoOwnWriteBase::Clear,
                TabTodoWriteBase::Edit => TabTodoOwnWriteBase::Edit,
            },
            generation,
            removals: plan
                .removals
                .iter()
                .map(|item| TabTodoItemRefRecord {
                    item_id: item.item_id.clone(),
                    expected_revision: item.expected_revision,
                })
                .collect(),
            appends: new_items(appended)?,
            state_changes,
        };
        checklist(
            self.repository
                .write_own(tab_id, workspace_id, team_id, at, write)
                .await
                .map_err(storage_error)?
                .value,
        )
    }

    pub async fn tab_ids_with_items(&self) -> Result<Vec<String>, String> {
        self.repository
            .tab_ids_with_items()
            .await
            .map(|read| read.value)
            .map_err(storage_error)
    }

    pub async fn forget_tab(&self, tab_id: String) -> Result<(), String> {
        self.repository
            .forget_tab(tab_id)
            .await
            .map(|_| ())
            .map_err(storage_error)
    }

    pub async fn forget_team(&self, team_id: String) -> Result<(), String> {
        self.repository
            .forget_team(team_id)
            .await
            .map(|_| ())
            .map_err(storage_error)
    }

    pub async fn forget_workspace(&self, workspace_id: String) -> Result<(), String> {
        self.repository
            .forget_workspace(workspace_id)
            .await
            .map(|_| ())
            .map_err(storage_error)
    }
}

fn new_items(items: Vec<TabTodoNewItem>) -> Result<Vec<TabTodoNewItemRecord>, String> {
    items
        .into_iter()
        .map(|item| {
            Ok(TabTodoNewItemRecord {
                id: item.id,
                title: item.title,
                description: item.description,
                link_json: encode_link(item.link)?,
            })
        })
        .collect()
}

fn checklist(record: TabTodoChecklistRecord) -> Result<TabTodoChecklist, String> {
    Ok(TabTodoChecklist {
        tab_id: record.tab_id,
        generation: record.generation,
        started_at: record.started_at,
        revision: record.revision,
        status: match record.status.as_str() {
            "active" => TabTodoChecklistStatus::Active,
            "complete" => TabTodoChecklistStatus::Complete,
            "cleared" => TabTodoChecklistStatus::Cleared,
            "closed" => TabTodoChecklistStatus::Closed,
            "reset" => TabTodoChecklistStatus::Reset,
            value => return Err(format!("storage returned unknown todo status {value}")),
        },
        items: record
            .items
            .into_iter()
            .map(item)
            .collect::<Result<Vec<_>, String>>()?,
    })
}

fn item(record: TabTodoItemRecord) -> Result<TabTodoItem, String> {
    let counters = &record.counters;
    Ok(TabTodoItem {
        state: parse_state(&record.state)?,
        link: record
            .link_json
            .map(|json| {
                serde_json::from_str::<TabTodoLink>(&json)
                    .map_err(|error| format!("storage returned malformed todo link: {error}"))
            })
            .transpose()?,
        attempt: narrow(record.attempt),
        counters: TabTodoCounters {
            started: narrow(counters.started),
            completed: narrow(counters.completed),
            reopened: narrow(counters.reopened),
            blocked: narrow(counters.blocked),
            skipped: narrow(counters.skipped),
            folded_events: narrow(counters.folded_events),
        },
        history: record
            .events
            .into_iter()
            .map(event)
            .collect::<Result<Vec<_>, String>>()?,
        id: record.id,
        position: record.position,
        title: record.title,
        description: record.description,
        revision: record.revision,
        locked: record.locked,
        reason: record.reason,
    })
}

fn event(record: TabTodoItemEventRecord) -> Result<TabTodoItemEvent, String> {
    Ok(TabTodoItemEvent {
        kind: match record.kind.as_str() {
            "started" => TabTodoEventKind::Started,
            "completed" => TabTodoEventKind::Completed,
            "reopened" => TabTodoEventKind::Reopened,
            "blocked" => TabTodoEventKind::Blocked,
            "skipped" => TabTodoEventKind::Skipped,
            value => return Err(format!("storage returned unknown todo event {value}")),
        },
        attempt: narrow(record.attempt),
        actor_tab_id: record.actor_tab_id,
        reason: record.reason,
        at: record.happened_at,
    })
}

fn parse_state(value: &str) -> Result<TabTodoState, String> {
    Ok(match value {
        "open" => TabTodoState::Open,
        "in_progress" => TabTodoState::InProgress,
        "done" => TabTodoState::Done,
        "blocked" => TabTodoState::Blocked,
        "skipped" => TabTodoState::Skipped,
        value => return Err(format!("storage returned unknown todo state {value}")),
    })
}

fn narrow(value: u64) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

fn encode_link(link: Option<TabTodoLink>) -> Result<Option<String>, String> {
    link.map(|link| serde_json::to_string(&link).map_err(|error| error.to_string()))
        .transpose()
}

fn state_string(state: TabTodoState) -> &'static str {
    match state {
        TabTodoState::Open => "open",
        TabTodoState::InProgress => "in_progress",
        TabTodoState::Done => "done",
        TabTodoState::Blocked => "blocked",
        TabTodoState::Skipped => "skipped",
    }
}

fn storage_error(error: codeterm_storage::StorageError) -> String {
    error.to_string()
}
