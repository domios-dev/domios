//! Storage-native tab todo records. DTO conversion belongs to the domain store.

use crate::error::StorageError;
use crate::repositories::to_sql_u64;
use std::collections::HashSet;

pub const MAX_TAB_TODO_ITEMS: usize = 16;
pub const MAX_TAB_TODO_AUDIT_EVENTS: usize = 64;
const MAX_TAB_ID_BYTES: usize = 256;
const MAX_ITEM_ID_BYTES: usize = 128;
const MAX_TEAM_ID_BYTES: usize = 256;
const MAX_LINK_JSON_BYTES: usize = 4096;
const RECORD_OVERHEAD: usize = 96;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabTodoItemRecord {
    pub id: String,
    pub position: u8,
    pub title: String,
    pub description: Option<String>,
    pub state: String,
    pub revision: u64,
    pub link_json: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabTodoChecklistRecord {
    pub tab_id: String,
    pub generation: u64,
    pub workspace_id: String,
    pub team_id: Option<String>,
    pub started_at: u64,
    pub revision: u64,
    pub status: String,
    pub items: Vec<TabTodoItemRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabTodoNewItemRecord {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub link_json: Option<String>,
}

impl TabTodoNewItemRecord {
    pub(crate) fn validate(&self) -> Result<(), StorageError> {
        identifier("todo item id", &self.id, MAX_ITEM_ID_BYTES)?;
        validate_title(&self.title)?;
        validate_description(self.description.as_deref())?;
        if self
            .link_json
            .as_ref()
            .is_some_and(|link| link.len() > MAX_LINK_JSON_BYTES)
        {
            return Err(StorageError::invalid(
                "todo link",
                "encoded link exceeds 4096 bytes",
            ));
        }
        Ok(())
    }

    pub(crate) fn cost(&self) -> usize {
        RECORD_OVERHEAD
            + self.id.len()
            + self.title.len()
            + self.description.as_ref().map_or(0, String::len)
            + self.link_json.as_ref().map_or(0, String::len)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabTodoAmendmentRecord {
    pub title: String,
    pub description: Option<String>,
    pub link_json: Option<String>,
}

impl TabTodoAmendmentRecord {
    pub(crate) fn validate(&self) -> Result<(), StorageError> {
        validate_title(&self.title)?;
        validate_description(self.description.as_deref())?;
        if self
            .link_json
            .as_ref()
            .is_some_and(|link| link.len() > MAX_LINK_JSON_BYTES)
        {
            return Err(StorageError::invalid(
                "todo link",
                "encoded link exceeds 4096 bytes",
            ));
        }
        Ok(())
    }

    pub(crate) fn cost(&self) -> usize {
        RECORD_OVERHEAD
            + self.title.len()
            + self.description.as_ref().map_or(0, String::len)
            + self.link_json.as_ref().map_or(0, String::len)
    }
}

pub(crate) fn validate_tab_id(value: &str) -> Result<(), StorageError> {
    identifier("todo tab id", value, MAX_TAB_ID_BYTES)
}

pub(crate) fn validate_team_id(value: Option<&str>) -> Result<(), StorageError> {
    value
        .map(|value| identifier("todo team id", value, MAX_TEAM_ID_BYTES))
        .unwrap_or(Ok(()))
}

pub(crate) fn validate_workspace_id(value: &str) -> Result<(), StorageError> {
    identifier("todo workspace id", value, MAX_TEAM_ID_BYTES)
}

pub(crate) fn validate_items(items: &[TabTodoNewItemRecord]) -> Result<usize, StorageError> {
    if items.is_empty() || items.len() > MAX_TAB_TODO_ITEMS {
        return Err(StorageError::invalid(
            "todo items",
            "must contain 1..=16 items",
        ));
    }
    let mut ids = HashSet::with_capacity(items.len());
    for item in items {
        item.validate()?;
        if !ids.insert(item.id.as_str()) {
            return Err(StorageError::invalid("todo items", "duplicate item id"));
        }
    }
    Ok(items.iter().map(TabTodoNewItemRecord::cost).sum())
}

pub(crate) fn validate_item_id(value: &str) -> Result<(), StorageError> {
    identifier("todo item id", value, MAX_ITEM_ID_BYTES)
}

fn validate_title(value: &str) -> Result<(), StorageError> {
    let chars = value.chars().count();
    if chars == 0 || chars > 120 || value.chars().any(char::is_control) {
        return Err(StorageError::invalid(
            "todo title",
            "must contain 1..=120 characters without controls",
        ));
    }
    Ok(())
}

fn validate_description(value: Option<&str>) -> Result<(), StorageError> {
    if value.is_some_and(|value| value.len() > 512) {
        return Err(StorageError::invalid(
            "todo description",
            "exceeds 512 bytes",
        ));
    }
    Ok(())
}

fn identifier(field: &'static str, value: &str, limit: usize) -> Result<(), StorageError> {
    if value.is_empty() || value.len() > limit || value.chars().any(char::is_control) {
        return Err(StorageError::invalid(
            field,
            format!("must contain 1..={limit} bytes without controls"),
        ));
    }
    Ok(())
}
