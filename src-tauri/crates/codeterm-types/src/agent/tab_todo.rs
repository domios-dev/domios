use serde::{Deserialize, Serialize};

pub const MAX_TAB_TODO_ITEMS: usize = 16;
pub const MAX_TAB_TODO_TITLE_CHARS: usize = 120;
pub const MAX_TAB_TODO_DESCRIPTION_BYTES: usize = 512;
pub const MAX_TAB_TODO_URL_BYTES: usize = 2048;
pub const MAX_TAB_TODO_SUMMARY_BYTES: usize = 512;
pub const MAX_TAB_TODO_SUMMARY_TITLES: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TabTodoLinkDraft {
    DomiosTask {
        task_ref: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        plan_ref: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
    },
    Url {
        url: String,
        label: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TabTodoLink {
    DomiosTask {
        plan_id: String,
        task_id: String,
        label: String,
    },
    Url {
        url: String,
        label: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabTodoDraft {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<TabTodoLinkDraft>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabTodoNewItem {
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<TabTodoLink>,
}

impl TabTodoDraft {
    pub fn validate(&self) -> Result<(), String> {
        let title_chars = self.title.chars().count();
        if self.title.trim().is_empty() || title_chars > MAX_TAB_TODO_TITLE_CHARS {
            return Err(format!(
                "todo title must contain 1..={MAX_TAB_TODO_TITLE_CHARS} characters"
            ));
        }
        if self.title.chars().any(char::is_control) {
            return Err("todo title must not contain control characters".into());
        }
        if self
            .description
            .as_ref()
            .is_some_and(|description| description.len() > MAX_TAB_TODO_DESCRIPTION_BYTES)
        {
            return Err(format!(
                "todo description exceeds {MAX_TAB_TODO_DESCRIPTION_BYTES} bytes"
            ));
        }
        if let Some(link) = &self.link {
            match link {
                TabTodoLinkDraft::DomiosTask {
                    task_ref,
                    plan_ref,
                    label,
                } => {
                    bounded_reference("todo task reference", task_ref)?;
                    if let Some(plan_ref) = plan_ref {
                        bounded_reference("todo plan reference", plan_ref)?;
                    }
                    if let Some(label) = label {
                        validate_label(label)?;
                    }
                }
                TabTodoLinkDraft::Url { url, label } => {
                    if url.is_empty() || url.len() > MAX_TAB_TODO_URL_BYTES {
                        return Err(format!(
                            "todo URL must contain 1..={MAX_TAB_TODO_URL_BYTES} bytes"
                        ));
                    }
                    validate_label(label)?;
                }
            }
        }
        Ok(())
    }
}

fn bounded_reference(field: &str, value: &str) -> Result<(), String> {
    if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
        return Err(format!(
            "{field} must contain 1..=256 bytes without controls"
        ));
    }
    Ok(())
}

fn validate_label(label: &str) -> Result<(), String> {
    let chars = label.chars().count();
    if label.trim().is_empty()
        || chars > MAX_TAB_TODO_TITLE_CHARS
        || label.chars().any(char::is_control)
    {
        return Err(format!(
            "todo link label must contain 1..={MAX_TAB_TODO_TITLE_CHARS} characters without controls"
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TabTodoState {
    Open,
    InProgress,
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TabTodoChecklistStatus {
    Active,
    Complete,
    Cleared,
    Closed,
    Reset,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabTodoItem {
    pub id: String,
    pub position: u8,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub state: TabTodoState,
    pub revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<TabTodoLink>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabTodoChecklist {
    pub tab_id: String,
    pub generation: u64,
    pub started_at: u64,
    pub revision: u64,
    pub status: TabTodoChecklistStatus,
    pub items: Vec<TabTodoItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabTodoSummary {
    pub tab_id: String,
    pub generation: u64,
    pub unfinished_count: u8,
    pub titles: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabTodoRecitedItem {
    pub id: String,
    pub title: String,
    pub state: TabTodoState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabTodoRecitation {
    pub tab_id: String,
    pub generation: u64,
    pub revision: u64,
    pub items: Vec<TabTodoRecitedItem>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draft_bounds_count_unicode_characters_and_description_bytes() {
        let mut draft = TabTodoDraft {
            title: "🦀".repeat(MAX_TAB_TODO_TITLE_CHARS),
            description: Some("я".repeat(MAX_TAB_TODO_DESCRIPTION_BYTES / 2)),
            link: None,
        };
        assert!(draft.validate().is_ok());
        draft.title.push('x');
        assert!(draft.validate().is_err());
        draft.title.pop();
        draft.description = Some("я".repeat(MAX_TAB_TODO_DESCRIPTION_BYTES / 2 + 1));
        assert!(draft.validate().is_err());
    }

    #[test]
    fn external_link_requires_bounded_label_and_url() {
        let draft = TabTodoDraft {
            title: "Review source".into(),
            description: None,
            link: Some(TabTodoLinkDraft::Url {
                url: "https://example.test/issue/1".into(),
                label: "Issue".into(),
            }),
        };
        assert!(draft.validate().is_ok());
    }
}
