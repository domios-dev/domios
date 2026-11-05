use super::Row;
use crate::plan::TaskStatus;
use std::collections::BTreeMap;

pub struct MetadataTaskInput<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub status: &'a TaskStatus,
    pub changed_at: u64,
    pub prior: Option<&'a str>,
    pub actionable: bool,
}

pub fn append_metadata_task_rows(
    rows: &mut BTreeMap<String, Vec<Row>>,
    input: MetadataTaskInput<'_>,
    since: u64,
) {
    if input.actionable {
        rows.entry("actionable".into()).or_default().push(Row {
            id: input.id.into(),
            kind: "task".into(),
            state: "waiting".into(),
            summary: String::new(),
        });
    }
    if input.changed_at > since {
        rows.entry("tasks moved".into()).or_default().push(Row {
            id: input.id.into(),
            kind: "task".into(),
            state: format!(
                "{} -> {}",
                input.prior.unwrap_or("unknown"),
                input.status.key()
            ),
            summary: input.name.into(),
        });
    }
}
