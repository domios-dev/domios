//! Workspace Radar: durable items noticed above any plan. An item stays
//! independent until it is pulled, which creates one linked plan task.

use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{NewTask, PlanError, PlanStore};
use crate::plan::PlanStatus;


const RADAR_DIR: &str = "radar";
const ID_PREFIX: &str = "rd";
const MAX_TITLE_CHARS: usize = 256;
const MAX_BODY_BYTES: usize = 64 * 1024;
const MAX_TAGS: usize = 32;
const MAX_IDENTITY_CHARS: usize = 256;

/// Serializes every Radar read-modify-write; item files are shared by all store handles.
static RADAR_WRITE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RadarStatus {
    #[default]
    Open,
    Pulled,
}

impl RadarStatus {
    pub fn key(self) -> &'static str {
        match self {
            RadarStatus::Open => "open",
            RadarStatus::Pulled => "pulled",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "open" => Some(RadarStatus::Open),
            "pulled" => Some(RadarStatus::Pulled),
            _ => None,
        }
    }
}

/// Identity of an item in the system it came from. Adding the same
/// `(provider, external_id)` again in one workspace refreshes that item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RadarSource {
    pub provider: String,
    pub external_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RadarPull {
    pub plan_id: String,
    pub task_id: String,
    pub pulled_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RadarItem {
    pub id: String,
    pub project_path: String,
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub status: RadarStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<RadarSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pulled: Option<RadarPull>,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Default)]
pub struct NewRadarItem {
    pub project_path: String,
    pub title: String,
    pub body: String,
    pub tags: Vec<String>,
    pub source: Option<RadarSource>,
}

#[derive(Debug, Clone, Default)]
pub struct RadarEdit {
    pub title: Option<String>,
    pub body: Option<String>,
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default)]
pub struct RadarFilter {
    pub project_path: Option<String>,
    /// Empty means every status.
    pub statuses: Vec<RadarStatus>,
    pub tag: Option<String>,
    pub provider: Option<String>,
    pub query: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct RadarPullRequest {
    pub item_id: String,
    pub plan_id: String,
    pub kind: Option<String>,
    pub section_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RadarAdded {
    pub item: RadarItem,
    /// False when an existing item with the same source identity was refreshed.
    pub created: bool,
}

#[derive(Debug)]
pub enum RadarError {
    NotFound(String),
    Invalid(String),
    AlreadyPulled(RadarPull),
    Plan(PlanError),
    Io(String),
}

impl fmt::Display for RadarError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RadarError::NotFound(id) => write!(f, "radar item '{id}' not found"),
            RadarError::Invalid(message) => write!(f, "{message}"),
            RadarError::AlreadyPulled(pull) => write!(
                f,
                "radar item was already pulled into plan {} as task {}",
                pull.plan_id, pull.task_id
            ),
            RadarError::Plan(error) => write!(f, "{error}"),
            RadarError::Io(message) => write!(f, "radar storage failed: {message}"),
        }
    }
}

impl std::error::Error for RadarError {}

fn io_error(error: impl fmt::Display) -> RadarError {
    RadarError::Io(error.to_string())
}

fn write_turn() -> std::sync::MutexGuard<'static, ()> {
    RADAR_WRITE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub fn is_radar_item_id(id: &str) -> bool {
    id.strip_prefix("rd_").is_some_and(|rest| {
        !rest.is_empty()
            && rest
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    })
}

fn clean_title(raw: &str) -> Result<String, RadarError> {
    let title = raw.trim();
    if title.is_empty() {
        return Err(RadarError::Invalid("radar item title is empty".into()));
    }
    if title.chars().count() > MAX_TITLE_CHARS || title.chars().any(char::is_control) {
        return Err(RadarError::Invalid(format!(
            "radar item title must be one line of at most {MAX_TITLE_CHARS} characters"
        )));
    }
    Ok(title.to_string())
}

fn clean_body(raw: &str) -> Result<String, RadarError> {
    if raw.len() > MAX_BODY_BYTES {
        return Err(RadarError::Invalid(format!(
            "radar item body exceeds {MAX_BODY_BYTES} bytes"
        )));
    }
    Ok(raw.to_string())
}

fn clean_tags(raw: Vec<String>) -> Result<Vec<String>, RadarError> {
    let mut tags: Vec<String> = Vec::new();
    for tag in raw {
        let tag = tag.trim().to_ascii_lowercase();
        if tag.is_empty() || tags.contains(&tag) {
            continue;
        }
        if tag.chars().count() > MAX_IDENTITY_CHARS || tag.chars().any(char::is_whitespace) {
            return Err(RadarError::Invalid(format!(
                "radar tag '{tag}' must be one word"
            )));
        }
        tags.push(tag);
    }
    if tags.len() > MAX_TAGS {
        return Err(RadarError::Invalid(format!(
            "a radar item carries at most {MAX_TAGS} tags"
        )));
    }
    Ok(tags)
}

fn clean_source(raw: RadarSource) -> Result<RadarSource, RadarError> {
    let provider = raw.provider.trim().to_ascii_lowercase();
    let provider_ok = !provider.is_empty()
        && provider.len() <= MAX_IDENTITY_CHARS
        && provider
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if !provider_ok {
        return Err(RadarError::Invalid(
            "radar source provider must be a non-empty id of letters, digits, '-', '_' or '.'"
                .into(),
        ));
    }
    let external_id = raw.external_id.trim().to_string();
    if external_id.is_empty()
        || external_id.chars().count() > MAX_IDENTITY_CHARS
        || external_id.chars().any(char::is_control)
    {
        return Err(RadarError::Invalid(
            "radar source external id must be a non-empty single line".into(),
        ));
    }
    let url = raw
        .url
        .map(|url| url.trim().to_string())
        .filter(|url| !url.is_empty());
    Ok(RadarSource {
        provider,
        external_id,
        url,
    })
}

fn matches_filter(item: &RadarItem, filter: &RadarFilter) -> bool {
    if filter
        .project_path
        .as_deref()
        .is_some_and(|project| item.project_path != project)
    {
        return false;
    }
    if !filter.statuses.is_empty() && !filter.statuses.contains(&item.status) {
        return false;
    }
    if filter
        .tag
        .as_deref()
        .is_some_and(|tag| !item.tags.iter().any(|t| t.eq_ignore_ascii_case(tag)))
    {
        return false;
    }
    if filter.provider.as_deref().is_some_and(|provider| {
        !item
            .source
            .as_ref()
            .is_some_and(|source| source.provider.eq_ignore_ascii_case(provider))
    }) {
        return false;
    }
    if let Some(query) = filter.query.as_deref() {
        let needle = query.to_lowercase();
        let haystack = format!("{}\n{}", item.title, item.body).to_lowercase();
        if !haystack.contains(&needle) {
            return false;
        }
    }
    true
}

/// File-per-item store; each handle shares the process-wide write lock.
#[derive(Debug, Clone)]
pub struct RadarStore {
    root: PathBuf,
}

impl RadarStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn item_path(&self, id: &str) -> PathBuf {
        self.root.join(format!("{id}.json"))
    }

    fn read(&self, id: &str) -> Result<RadarItem, RadarError> {
        if !is_radar_item_id(id) {
            return Err(RadarError::NotFound(id.to_string()));
        }
        let raw = match std::fs::read_to_string(self.item_path(id)) {
            Ok(raw) => raw,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(RadarError::NotFound(id.to_string()))
            }
            Err(error) => return Err(io_error(error)),
        };
        serde_json::from_str(&raw).map_err(io_error)
    }

    fn write(&self, item: &RadarItem) -> Result<(), RadarError> {
        std::fs::create_dir_all(&self.root).map_err(io_error)?;
        let bytes = serde_json::to_vec_pretty(item).map_err(io_error)?;
        super::atomic_write::write_atomic(&self.item_path(&item.id), bytes).map_err(io_error)
    }

    fn all(&self) -> Result<Vec<RadarItem>, RadarError> {
        let entries = match std::fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(io_error(error)),
        };
        let mut items = Vec::new();
        for entry in entries {
            let path = entry.map_err(io_error)?.path();
            let Some(id) = path
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.strip_suffix(".json"))
            else {
                continue;
            };
            if !is_radar_item_id(id) {
                continue;
            }
            match self.read(id) {
                Ok(item) => items.push(item),
                Err(RadarError::NotFound(_)) => {}
                Err(error) => log::warn!("[radar] skipped unreadable item {id}: {error}"),
            }
        }
        Ok(items)
    }

    fn mint_id(&self) -> String {
        crate::plan::ids::mint_id(ID_PREFIX, |candidate| self.item_path(candidate).exists())
    }

    pub fn get(&self, id: &str) -> Result<RadarItem, RadarError> {
        self.read(id)
    }

    /// Newest first, so the latest notice leads the list. The workspace path may be any directory inside it.
    pub fn list(&self, filter: &RadarFilter) -> Result<Vec<RadarItem>, RadarError> {
        let filter = RadarFilter {
            project_path: filter
                .project_path
                .as_deref()
                .map(super::task_queries::canonical_project),
            ..filter.clone()
        };
        let mut items: Vec<RadarItem> = self
            .all()?
            .into_iter()
            .filter(|item| matches_filter(item, &filter))
            .collect();
        items.sort_by(|a, b| {
            b.created_at
                .cmp(&a.created_at)
                .then_with(|| a.id.cmp(&b.id))
        });
        Ok(items)
    }

    pub fn add(&self, new: NewRadarItem, now_ms: u64) -> Result<RadarAdded, RadarError> {
        if new.project_path.trim().is_empty() {
            return Err(RadarError::Invalid(
                "a radar item needs a workspace project path".into(),
            ));
        }
        let project_path = super::task_queries::canonical_project(&new.project_path);
        let title = clean_title(&new.title)?;
        let body = clean_body(&new.body)?;
        let tags = clean_tags(new.tags)?;
        let source = new.source.map(clean_source).transpose()?;
        let _turn = write_turn();
        if let Some(source) = source.as_ref() {
            let existing = self.all()?.into_iter().find(|item| {
                item.project_path == project_path
                    && item.source.as_ref().is_some_and(|known| {
                        known.provider == source.provider && known.external_id == source.external_id
                    })
            });
            if let Some(mut item) = existing {
                item.title = title;
                item.body = body;
                item.tags = tags;
                item.source = Some(source.clone());
                item.updated_at = now_ms;
                self.write(&item)?;
                return Ok(RadarAdded {
                    item,
                    created: false,
                });
            }
        }
        let item = RadarItem {
            id: self.mint_id(),
            project_path,
            title,
            body,
            tags,
            status: RadarStatus::Open,
            source,
            pulled: None,
            created_at: now_ms,
            updated_at: now_ms,
        };
        self.write(&item)?;
        Ok(RadarAdded {
            item,
            created: true,
        })
    }

    pub fn update(&self, id: &str, edit: RadarEdit, now_ms: u64) -> Result<RadarItem, RadarError> {
        if edit.title.is_none() && edit.body.is_none() && edit.tags.is_none() {
            return Err(RadarError::Invalid(
                "radar edit needs at least one of title, body or tags".into(),
            ));
        }
        let title = edit.title.as_deref().map(clean_title).transpose()?;
        let body = edit.body.as_deref().map(clean_body).transpose()?;
        let tags = edit.tags.map(clean_tags).transpose()?;
        let _turn = write_turn();
        let mut item = self.read(id)?;
        if let Some(title) = title {
            item.title = title;
        }
        if let Some(body) = body {
            item.body = body;
        }
        if let Some(tags) = tags {
            item.tags = tags;
        }
        item.updated_at = now_ms;
        self.write(&item)?;
        Ok(item)
    }

    /// Removes the item for good; a task it was pulled into stays in its plan.
    pub fn drop_item(&self, id: &str) -> Result<RadarItem, RadarError> {
        let _turn = write_turn();
        let item = self.read(id)?;
        std::fs::remove_file(self.item_path(id)).map_err(io_error)?;
        Ok(item)
    }
}

impl PlanStore {
    pub fn radar(&self) -> RadarStore {
        let database_path = self.database.path();
        RadarStore::new(
            database_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join(RADAR_DIR),
        )
    }

    /// Creates one task in a plan of the item's own workspace and links it back.
    pub fn pull_radar_item(
        &self,
        request: RadarPullRequest,
        now_ms: u64,
    ) -> Result<RadarItem, RadarError> {
        let radar = self.radar();
        let _turn = write_turn();
        let mut item = radar.read(&request.item_id)?;
        if let Some(pull) = item.pulled.clone() {
            return Err(RadarError::AlreadyPulled(pull));
        }
        let plan = self
            .get(&request.plan_id)
            .ok_or_else(|| RadarError::Plan(PlanError::NotFound))?;
        if plan.status == PlanStatus::Archived {
            return Err(RadarError::Invalid(format!(
                "plan {} is archived; pull into an active plan",
                plan.id
            )));
        }
        if super::task_queries::canonical_project(&plan.project_path) != item.project_path {
            return Err(RadarError::Invalid(format!(
                "plan {} belongs to another workspace than radar item {}",
                plan.id, item.id
            )));
        }
        let external_refs: Vec<String> = item
            .source
            .as_ref()
            .and_then(|source| source.url.clone())
            .filter(|url| super::external_refs::canonicalize_external_ref(url).is_ok())
            .into_iter()
            .collect();
        let mut new_task = NewTask {
            name: item.title.clone(),
            body: item.body.clone(),
            section_ids: request.section_ids,
            ..Default::default()
        };
        if let Some(kind) = request.kind.filter(|kind| !kind.trim().is_empty()) {
            new_task.kind = kind;
        }
        let task_id = self
            .add_task_with_external_refs(&plan.id, new_task, &external_refs, now_ms)
            .map_err(RadarError::Plan)?;
        item.status = RadarStatus::Pulled;
        item.pulled = Some(RadarPull {
            plan_id: plan.id.clone(),
            task_id: task_id.clone(),
            pulled_at: now_ms,
        });
        item.updated_at = now_ms;
        radar.write(&item).map_err(|error| {
            RadarError::Io(format!(
                "task {task_id} was created in plan {} but linking radar item {} failed: {error}",
                plan.id, item.id
            ))
        })?;
        Ok(item)
    }
}

#[cfg(test)]
mod tests;
