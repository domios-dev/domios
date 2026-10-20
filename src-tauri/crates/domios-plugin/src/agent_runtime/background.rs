use super::{RuntimeContext, RuntimeFailure, RuntimeFailureCode};
use serde::{Deserialize, Serialize};

pub const MAX_BACKGROUND_PROCESSES: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeBackgroundProcess {
    pub process_id: String,
    pub item_id: String,
    pub command: String,
    pub cwd: String,
    pub os_pid: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum RuntimeBackgroundProcessAction {
    List { cursor: Option<String> },
    Terminate { process_id: String },
    Clean,
}

pub fn owns_runtime_thread(context: &RuntimeContext, thread: &str) -> bool {
    let Some(root) = context.thread_id.as_deref() else {
        return false;
    };
    let mut current = thread;
    let mut seen = std::collections::HashSet::new();
    while current != root {
        if !seen.insert(current) {
            return false;
        }
        let Some(parent) = context
            .thread_parents
            .iter()
            .find(|parent| parent.thread_id == current)
        else {
            return false;
        };
        current = &parent.parent_thread_id;
    }
    true
}

pub fn validate_background_processes(
    processes: &[RuntimeBackgroundProcess],
) -> Result<(), RuntimeFailure> {
    let mut ids = std::collections::HashSet::new();
    let identifier =
        |value: &str| !value.is_empty() && value.len() <= 4096 && !value.contains('\0');
    if processes.len() > MAX_BACKGROUND_PROCESSES
        || processes.iter().any(|process| {
            !identifier(&process.process_id)
                || !identifier(&process.item_id)
                || !identifier(&process.cwd)
                || process.command.len() > 65536
                || process.command.contains('\0')
                || !ids.insert(&process.process_id)
        })
    {
        return Err(RuntimeFailure::new(
            RuntimeFailureCode::InvalidSchema,
            "invalid background process roster",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
