use codeterm_storage::{
    TabTodoAmendmentRecord, TabTodoChecklistRecord, TabTodoNewItemRecord, TabTodoRepository,
};
use codeterm_types::agent::{
    TabTodoChecklist, TabTodoChecklistStatus, TabTodoItem, TabTodoLink, TabTodoNewItem,
    TabTodoState,
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
                    actor,
                    at,
                )
                .await
                .map_err(storage_error)?
                .value,
        )
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
            .map(|item| {
                Ok(TabTodoItem {
                    id: item.id,
                    position: item.position,
                    title: item.title,
                    description: item.description,
                    state: match item.state.as_str() {
                        "open" => TabTodoState::Open,
                        "in_progress" => TabTodoState::InProgress,
                        "done" => TabTodoState::Done,
                        value => {
                            return Err(format!("storage returned unknown todo state {value}"))
                        }
                    },
                    revision: item.revision,
                    link: item
                        .link_json
                        .map(|json| {
                            serde_json::from_str::<TabTodoLink>(&json).map_err(|error| {
                                format!("storage returned malformed todo link: {error}")
                            })
                        })
                        .transpose()?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?,
    })
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
    }
}

fn storage_error(error: codeterm_storage::StorageError) -> String {
    error.to_string()
}
