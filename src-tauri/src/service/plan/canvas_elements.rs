use super::mutations::gen_id;
use super::{PlanError, PlanStore};
use crate::plan::{CanvasElement, ElementCanvas, Section};

fn set_section_canvas(sections: &mut [Section], id: &str, x: Option<f64>, y: Option<f64>) -> bool {
    for section in sections.iter_mut() {
        if section.id == id {
            section.canvas.x = x;
            section.canvas.y = y;
            return true;
        }
        if set_section_canvas(&mut section.subsections, id, x, y) {
            return true;
        }
    }
    false
}

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
        self.with_plan(plan_id, now_ms, |p| {
            p.elements.push(el);
            Ok(())
        })?;
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
        self.with_plan(plan_id, now_ms, |p| {
            p.elements.push(el);
            Ok(())
        })?;
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
        self.with_plan(plan_id, now_ms, |p| {
            let element = p
                .elements
                .iter_mut()
                .find(|e| e.id == element_id)
                .ok_or(PlanError::ElementNotFound)?;
            if let Some(kind) = kind {
                element.kind = kind.into();
            }
            if let Some(content) = content {
                element.content = content;
            }
            if let Some(links) = links {
                element.links = links;
            }
            Ok(())
        })
    }

    pub fn rm_element(
        &self,
        plan_id: &str,
        element_id: &str,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_plan(plan_id, now_ms, |p| {
            let pos = p
                .elements
                .iter()
                .position(|e| e.id == element_id)
                .ok_or(PlanError::ElementNotFound)?;
            p.elements.remove(pos);
            Ok(())
        })
    }

    pub fn set_layout(
        &self,
        plan_id: &str,
        element_id: &str,
        x: Option<f64>,
        y: Option<f64>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_plan(plan_id, now_ms, |p| {
            let element_id = normalize_layout_element_id(element_id);
            let found = set_section_canvas(&mut p.sections, element_id, x, y)
                || p.tasks
                    .iter_mut()
                    .find(|t| t.id == element_id)
                    .is_some_and(|t| {
                        t.canvas.x = x;
                        t.canvas.y = y;
                        true
                    })
                || p.tasks
                    .iter_mut()
                    .flat_map(|t| t.milestones.iter_mut())
                    .find(|m| m.id == element_id)
                    .is_some_and(|m| {
                        m.canvas.x = x;
                        m.canvas.y = y;
                        true
                    })
                || p.elements
                    .iter_mut()
                    .find(|e| e.id == element_id)
                    .is_some_and(|e| {
                        e.canvas.x = x;
                        e.canvas.y = y;
                        true
                    });
            if found {
                Ok(())
            } else {
                Err(PlanError::NotFound)
            }
        })
    }
}
