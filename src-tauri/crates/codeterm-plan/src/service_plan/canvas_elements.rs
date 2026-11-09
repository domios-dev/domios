use super::mutations::gen_id;
use super::store_db::hot_write::ElementChange;
use super::{PlanError, PlanStore};
use crate::plan::{CanvasElement, ElementCanvas};

fn normalize_layout_element_id(element_id: &str) -> &str {
    element_id
        .strip_prefix("task:")
        .or_else(|| element_id.strip_prefix("section:"))
        .or_else(|| element_id.strip_prefix("sec:"))
        .or_else(|| element_id.strip_prefix("milestone:"))
        .or_else(|| element_id.strip_prefix("ms:"))
        .or_else(|| element_id.strip_prefix("element:"))
        .or_else(|| element_id.strip_prefix("el:"))
        .unwrap_or(element_id)
}

impl PlanStore {
    pub fn add_note(
        &self,
        plan_id: &str,
        text: &str,
        links: Vec<String>,
        x: Option<f64>,
        y: Option<f64>,
        now_ms: u64,
    ) -> Result<String, PlanError> {
        let eid = gen_id("el");
        let el = CanvasElement {
            id: eid.clone(),
            kind: "note".into(),
            canvas: ElementCanvas {
                x,
                y,
                collapsed: false,
            },
            content: serde_json::json!({ "text": text }),
            links,
        };
        self.commit_element_change(plan_id, ElementChange::Add(el), now_ms)?;
        Ok(eid)
    }

    pub fn add_element(
        &self,
        plan_id: &str,
        kind: &str,
        content: serde_json::Value,
        links: Vec<String>,
        x: Option<f64>,
        y: Option<f64>,
        now_ms: u64,
    ) -> Result<String, PlanError> {
        let eid = gen_id("el");
        let el = CanvasElement {
            id: eid.clone(),
            kind: kind.into(),
            canvas: ElementCanvas {
                x,
                y,
                collapsed: false,
            },
            content,
            links,
        };
        self.commit_element_change(plan_id, ElementChange::Add(el), now_ms)?;
        Ok(eid)
    }

    pub fn update_element(
        &self,
        plan_id: &str,
        element_id: &str,
        kind: Option<&str>,
        content: Option<serde_json::Value>,
        links: Option<Vec<String>>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.commit_element_change(
            plan_id,
            ElementChange::Update {
                id: element_id.into(),
                kind: kind.map(str::to_string),
                content,
                links,
            },
            now_ms,
        )
    }

    pub fn rm_element(
        &self,
        plan_id: &str,
        element_id: &str,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.commit_element_change(
            plan_id,
            ElementChange::Remove {
                id: element_id.into(),
            },
            now_ms,
        )
    }

    pub fn set_layout(
        &self,
        plan_id: &str,
        element_id: &str,
        x: Option<f64>,
        y: Option<f64>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.commit_element_change(
            plan_id,
            ElementChange::Layout {
                id: normalize_layout_element_id(element_id).into(),
                x,
                y,
            },
            now_ms,
        )
    }

    fn commit_element_change(
        &self,
        plan_id: &str,
        change: ElementChange,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        let turn = self.mutation_writer.enter();
        let receipt = self.database.change_element(plan_id, change, now_ms)?;
        self.publish_hot_receipt(&turn, receipt);
        Ok(())
    }
}
