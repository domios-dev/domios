use serde::{Deserialize, Serialize};

pub const INBOX_PLAN_ID: &str = "inbox";
pub const INBOX_PLAN_FILE_STEM: &str = "_inbox";
pub const INBOX_PLAN_TITLE: &str = "Tasks";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PlanStatus {
    Active,
    Archived,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    InProgress,
    Blocked,
    Done,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum TaskPriority {
    P0,
    P1,
    #[default]
    P2,
    P3,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum LoopStatus {
    Running,
    Done,
    Exhausted,
    Stopped,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ElementCanvas {
    pub x: Option<f64>,
    pub y: Option<f64>,
    #[serde(default)]
    pub collapsed: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct OrchestratorRef {
    pub pane_id: Option<String>,
    pub session_id: Option<String>,
    pub title: Option<String>,
}

impl OrchestratorRef {
    pub fn from_pane(p: &str) -> Self {
        Self {
            pane_id: Some(p.into()),
            ..Default::default()
        }
    }
}

pub type AgentRef = OrchestratorRef;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TransferRecord {
    pub from: OrchestratorRef,
    pub to: OrchestratorRef,
    pub ts: u64,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MilestoneKind {
    pub key: String,
    pub label: String,
    pub color: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TaskGroup {
    pub key: String,
    pub label: String,
    pub color: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct TagFacetValue {
    pub v: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct TagFacet {
    pub key: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub color_lane: bool,
    #[serde(default)]
    pub values: Vec<TagFacetValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Section {
    pub id: String,
    #[serde(alias = "name")]
    pub title: String,
    #[serde(default)]
    pub handle: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub subsections: Vec<Section>,
    #[serde(default)]
    pub canvas: ElementCanvas,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Milestone {
    pub id: String,
    pub from: AgentRef,
    pub kind: String,
    #[serde(default)]
    pub text: String,
    pub ts: u64,
    #[serde(default)]
    pub links: Vec<String>,
    pub loop_id: Option<String>,
    pub iteration: Option<u32>,
    #[serde(default)]
    pub canvas: ElementCanvas,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Task {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub model: Option<String>,
    pub model_rationale: Option<String>,
    pub status: TaskStatus,
    pub status_label: Option<String>,
    #[serde(default)]
    pub priority: TaskPriority,
    #[serde(default)]
    pub created_at: u64,
    #[serde(default)]
    pub updated_at: u64,
    #[serde(default)]
    pub status_changed_at: u64,
    #[serde(default)]
    pub handle: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub section_ids: Vec<String>,
    #[serde(default)]
    pub body: String,
    pub intent: Option<String>,
    #[serde(default)]
    pub parent_task_id: Option<String>,
    pub assigned_pane_id: Option<String>,
    #[serde(default)]
    pub taken_by_role: Option<String>,
    pub peer: Option<String>,
    #[serde(default)]
    pub group: Option<String>,
    #[serde(default)]
    pub canvas: ElementCanvas,
    #[serde(default)]
    pub milestones: Vec<Milestone>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LoopStage {
    pub key: String,
    pub role: String,
    pub model: Option<String>,
    pub why: Option<String>,
    pub prompt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LoopSpec {
    pub id: String,
    pub target: String,
    #[serde(default)]
    pub stages: Vec<LoopStage>,
    pub budget: u32,
    #[serde(default)]
    pub count: u32,
    pub stop_when: Option<String>,
    pub status: LoopStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CanvasElement {
    pub id: String,
    pub kind: String,
    #[serde(default)]
    pub canvas: ElementCanvas,
    #[serde(default)]
    pub content: serde_json::Value,
    #[serde(default)]
    pub links: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Viewport {
    pub pan_x: f64,
    pub pan_y: f64,
    pub scale: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct PlanCanvas {
    pub viewport: Option<Viewport>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SpecLink {
    pub spec_id: String,
    pub spec_section_id: String,
    pub plan_section_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum EffortLinkRel {
    Follows,
    Supersedes,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EffortLink {
    pub to: String,
    pub rel: EffortLinkRel,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Spec {
    pub id: String,
    pub title: String,
    pub project_path: String,
    pub status: PlanStatus,
    #[serde(default)]
    pub sections: Vec<Section>,
    pub created_at: u64,
    pub updated_at: u64,
    #[serde(default)]
    pub canvas: PlanCanvas,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Plan {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub sections: Vec<Section>,
    pub status: PlanStatus,
    pub owner: OrchestratorRef,
    pub project_path: String,
    pub created_at: u64,
    pub updated_at: u64,
    #[serde(default)]
    pub milestone_kinds: Vec<MilestoneKind>,
    #[serde(default)]
    pub loops: Vec<LoopSpec>,
    #[serde(default)]
    pub elements: Vec<CanvasElement>,
    #[serde(default)]
    pub tasks: Vec<Task>,
    #[serde(default)]
    pub task_groups: Vec<TaskGroup>,
    #[serde(default)]
    pub tag_facets: Vec<TagFacet>,
    #[serde(default)]
    pub links: Vec<String>,
    #[serde(default)]
    pub spec_refs: Vec<String>,
    #[serde(default)]
    pub spec_links: Vec<SpecLink>,
    #[serde(default)]
    pub primary_spec_id: Option<String>,
    #[serde(default)]
    pub lineage: Vec<EffortLink>,
    #[serde(default)]
    pub canvas: PlanCanvas,
    #[serde(default)]
    pub owner_history: Vec<TransferRecord>,
}

impl Plan {
    pub fn new(title: String, project_path: String, owner: OrchestratorRef) -> Self {
        let id = format!("pl_{}", uuid::Uuid::new_v4().simple());
        Self {
            id,
            title,
            sections: vec![],
            status: PlanStatus::Active,
            owner,
            project_path,
            created_at: 0,
            updated_at: 0,
            milestone_kinds: vec![],
            loops: vec![],
            elements: vec![],
            tasks: vec![],
            task_groups: vec![],
            tag_facets: vec![],
            links: vec![],
            spec_refs: vec![],
            spec_links: vec![],
            primary_spec_id: None,
            lineage: vec![],
            canvas: PlanCanvas::default(),
            owner_history: vec![],
        }
    }
}

impl Spec {
    pub fn new(title: String, project_path: String) -> Self {
        Self {
            id: format!("sp_{}", uuid::Uuid::new_v4().simple()),
            title,
            project_path,
            status: PlanStatus::Active,
            sections: vec![],
            created_at: 0,
            updated_at: 0,
            canvas: PlanCanvas::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_roundtrips_and_defaults() {
        let p = Plan::new(
            "Voice UX".into(),
            "/repo".into(),
            OrchestratorRef::from_pane("abc123"),
        );
        assert_eq!(p.status, PlanStatus::Active);
        assert!(p.sections.is_empty() && p.tasks.is_empty() && p.loops.is_empty());
        let json = serde_json::to_string(&p).unwrap();
        let back: Plan = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, p.id);
        assert_eq!(back.title, "Voice UX");
    }

    #[test]
    fn task_groups_and_links_roundtrip() {
        let mut p = Plan::new("T".into(), "/repo".into(), Default::default());
        p.task_groups = vec![TaskGroup {
            key: "phase-1".into(),
            label: "Phase 1".into(),
            color: None,
        }];
        p.links = vec!["pl_other".into()];
        let json = serde_json::to_string(&p).unwrap();
        let back: Plan = serde_json::from_str(&json).unwrap();
        assert_eq!(back.task_groups.len(), 1);
        assert_eq!(back.task_groups[0].key, "phase-1");
        assert_eq!(back.links, vec!["pl_other".to_string()]);
    }

    #[test]
    fn old_json_without_new_fields_still_loads() {
        // A plan file written before task_groups / links / Task.group existed.
        let raw = r#"{
            "id": "pl_old",
            "title": "Legacy",
            "status": "active",
            "owner": {},
            "project_path": "/repo",
            "created_at": 1,
            "updated_at": 2,
            "tasks": [
                {"id":"task_1","name":"n","type":"implement","model":null,
                 "model_rationale":null,"status":"pending","status_label":null,
                 "section_ids":[],"body":"","intent":null,"assigned_pane_id":null,
                 "peer":null}
            ]
        }"#;
        let p: Plan = serde_json::from_str(raw).unwrap();
        assert_eq!(p.id, "pl_old");
        assert!(p.task_groups.is_empty());
        assert!(p.tag_facets.is_empty());
        assert!(p.links.is_empty());
        assert_eq!(p.tasks.len(), 1);
        assert_eq!(p.tasks[0].group, None);
        assert_eq!(p.tasks[0].priority, TaskPriority::P2);
        assert_eq!(p.tasks[0].created_at, 0);
        assert_eq!(p.tasks[0].updated_at, 0);
        assert_eq!(p.tasks[0].status_changed_at, 0);
        assert_eq!(p.tasks[0].handle, None);
        assert!(p.tasks[0].tags.is_empty());
        assert_eq!(p.tasks[0].taken_by_role, None);
        assert_eq!(p.tasks[0].parent_task_id, None);
    }

    #[test]
    fn task_status_is_closed_enum() {
        let t: Result<TaskStatus, _> = serde_json::from_str("\"reviewing\"");
        assert!(t.is_err(), "unknown status must reject");
        let ok: TaskStatus = serde_json::from_str("\"in_progress\"").unwrap();
        assert_eq!(ok, TaskStatus::InProgress);
    }

    #[test]
    fn task_priority_is_closed_enum() {
        let bad: Result<TaskPriority, _> = serde_json::from_str("\"urgent\"");
        assert!(bad.is_err(), "unknown priority must reject");
        let ok: TaskPriority = serde_json::from_str("\"p1\"").unwrap();
        assert_eq!(ok, TaskPriority::P1);
    }
}
