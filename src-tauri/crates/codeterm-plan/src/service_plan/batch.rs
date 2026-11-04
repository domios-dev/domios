use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{NewTask, PlanError, PlanStore};
use crate::plan::{AgentRef, IdentityProvenance};

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum BatchOp {
    UpsertTask {
        #[serde(default)]
        client_id: Option<String>,
        handle: String,
        #[serde(default)]
        name: Option<String>,
        #[serde(rename = "type", default)]
        kind: Option<String>,
        #[serde(default)]
        sections: Vec<String>,
        #[serde(default)]
        status: Option<String>,
        #[serde(default)]
        body: Option<String>,
        #[serde(default)]
        model: Option<String>,
        #[serde(default)]
        why: Option<String>,
        #[serde(default)]
        priority: Option<String>,
        #[serde(default)]
        group: Option<String>,
        #[serde(default)]
        personality: Option<String>,
        #[serde(default)]
        personality_provenance: Option<IdentityProvenance>,
    },
    SetTaskBody {
        task: String,
        #[serde(alias = "body_md")]
        body: String,
    },
    IngestSections {
        #[serde(alias = "markdown")]
        body: String,
    },
    SetTaskSections {
        #[serde(default)]
        client_id: Option<String>,
        task: String,
        #[serde(default)]
        sections: Vec<String>,
    },
    SetTaskStatus {
        #[serde(default)]
        client_id: Option<String>,
        task: String,
        status: String,
        #[serde(default)]
        status_label: Option<String>,
    },
    SetGoal {
        #[serde(default)]
        client_id: Option<String>,
        vector: String,
        #[serde(default)]
        vision: String,
        #[serde(default)]
        constraints: Vec<String>,
        #[serde(default)]
        postulates: Vec<String>,
    },
    AddMilestone {
        #[serde(default)]
        client_id: Option<String>,
        task: String,
        kind: String,
        #[serde(default)]
        text: String,
        #[serde(default)]
        from: Option<AgentRef>,
        #[serde(default)]
        key: Option<String>,
        #[serde(default)]
        links: Vec<String>,
    },
    AddElement {
        #[serde(default)]
        client_id: Option<String>,
        kind: String,
        #[serde(default)]
        content: Value,
        #[serde(default)]
        link: Option<String>,
        #[serde(default)]
        links: Vec<String>,
        #[serde(default)]
        x: Option<f64>,
        #[serde(default)]
        y: Option<f64>,
    },
    AddHandoff {
        #[serde(default)]
        client_id: Option<String>,
        #[serde(default)]
        mode: Option<String>,
        #[serde(default)]
        content: Value,
        #[serde(alias = "body_md", default)]
        body: String,
        #[serde(default)]
        task: Option<String>,
        #[serde(default)]
        from: Option<AgentRef>,
        #[serde(default)]
        to: Option<AgentRef>,
    },
    Link {
        to: String,
    },
    UpsertSpec {
        #[serde(default)]
        client_id: Option<String>,
        #[serde(default)]
        spec_id: Option<String>,
        title: String,
    },
    UpsertSpecSection {
        #[serde(default)]
        client_id: Option<String>,
        spec: String,
        #[serde(default)]
        section_id: Option<String>,
        title: String,
        #[serde(default)]
        body: Option<String>,
    },
    SetSpecSectionBody {
        spec: String,
        section: String,
        #[serde(alias = "body_md")]
        body: String,
    },
    LinkSpecSection {
        spec: String,
        spec_section: String,
        plan_section: String,
    },
    UnlinkSpecSection {
        spec: String,
        #[serde(default)]
        spec_section: Option<String>,
        #[serde(default)]
        plan_section: Option<String>,
    },
    CreatePlanFromSpec {
        #[serde(default)]
        client_id: Option<String>,
        spec: String,
        title: String,
    },
}

#[derive(Debug, Clone, Deserialize)]
pub struct BatchRequest {
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub ops: Vec<BatchOp>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct BatchResult {
    pub ok: bool,
    pub ids: HashMap<String, String>,
    pub task_ids: Vec<String>,
    pub milestone_ids: Vec<String>,
    pub element_ids: Vec<String>,
    pub spec_ids: Vec<String>,
    pub spec_section_ids: Vec<String>,
    #[serde(rename = "_hints", skip_serializing_if = "Vec::is_empty")]
    pub hints: Vec<String>,
}

impl Default for BatchResult {
    fn default() -> Self {
        Self {
            ok: true,
            ids: HashMap::new(),
            task_ids: Vec::new(),
            milestone_ids: Vec::new(),
            element_ids: Vec::new(),
            spec_ids: Vec::new(),
            spec_section_ids: Vec::new(),
            hints: Vec::new(),
        }
    }
}

impl PlanStore {
    pub fn apply_batch(
        &self,
        plan_id: &str,
        request: BatchRequest,
        now_ms: u64,
    ) -> Result<BatchResult, PlanError> {
        self.commit_hot_batch(plan_id, request, now_ms)
    }
}

impl From<NewTask> for BatchOp {
    fn from(task: NewTask) -> Self {
        BatchOp::UpsertTask {
            client_id: None,
            handle: task.handle.unwrap_or(task.name.clone()),
            name: Some(task.name),
            kind: Some(task.kind),
            sections: task.section_ids,
            status: None,
            body: Some(task.body),
            model: task.model,
            why: task.model_rationale,
            priority: task.priority.map(|p| {
                serde_json::to_value(p)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_string))
                    .unwrap_or_else(|| "p2".into())
            }),
            group: task.group,
            personality: task.personality,
            personality_provenance: task.personality_provenance,
        }
    }
}

#[cfg(test)]
mod tests;
