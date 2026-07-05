use serde::Serialize;
use serde_json::Value;

use super::{PlanError, PlanStore};
use crate::plan::{CanvasElement, ElementCanvas};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ElementSummary {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub links: Vec<String>,
    pub canvas: ElementCanvas,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ElementList {
    pub plan_id: String,
    pub elements: Vec<ElementSummary>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ElementDetails {
    pub plan_id: String,
    pub element: CanvasElement,
    pub title: String,
    pub markdown: String,
}

impl PlanStore {
    pub fn list_elements(
        &self,
        plan_id: &str,
        kind: Option<&str>,
    ) -> Result<ElementList, PlanError> {
        let plan = self.get(plan_id).ok_or(PlanError::NotFound)?;
        let kind = kind.map(str::trim).filter(|value| !value.is_empty());
        let elements = plan
            .elements
            .iter()
            .filter(|element| kind.is_none_or(|want| element.kind == want))
            .map(element_summary)
            .collect();
        Ok(ElementList {
            plan_id: plan.id,
            elements,
        })
    }

    pub fn show_element(
        &self,
        plan_id: &str,
        element_id: &str,
    ) -> Result<ElementDetails, PlanError> {
        let plan = self.get(plan_id).ok_or(PlanError::NotFound)?;
        let element = plan
            .elements
            .iter()
            .find(|element| element.id == element_id)
            .cloned()
            .ok_or(PlanError::ElementNotFound)?;
        let title = element_title(&element);
        let markdown = element_markdown(&element);
        Ok(ElementDetails {
            plan_id: plan.id,
            element,
            title,
            markdown,
        })
    }
}

fn element_summary(element: &CanvasElement) -> ElementSummary {
    ElementSummary {
        id: element.id.clone(),
        kind: element.kind.clone(),
        title: element_title(element),
        links: element.links.clone(),
        canvas: element.canvas.clone(),
    }
}

fn element_title(element: &CanvasElement) -> String {
    for key in ["title", "name", "label"] {
        if let Some(value) = string_field(&element.content, key) {
            return value;
        }
    }
    for key in ["text", "body", "markdown"] {
        if let Some(value) = string_field(&element.content, key).and_then(first_non_empty_line) {
            return value;
        }
    }
    element.id.clone()
}

fn element_markdown(element: &CanvasElement) -> String {
    for key in ["markdown", "body", "text"] {
        if let Some(value) = string_field(&element.content, key) {
            return value;
        }
    }
    serde_json::to_string_pretty(&element.content).unwrap_or_else(|_| "{}".into())
}

fn string_field(content: &Value, key: &str) -> Option<String> {
    content
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn first_non_empty_line(value: String) -> Option<String> {
    value
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{OrchestratorRef, Plan};
    use serde_json::json;

    fn store_with_elements() -> (tempfile::TempDir, PlanStore, String, String) {
        let dir = tempfile::tempdir().unwrap();
        let store = PlanStore::new_for_test(dir.path());
        let mut plan = Plan::new(
            "Elements".into(),
            dir.path().display().to_string(),
            OrchestratorRef::default(),
        );
        plan.id = "pl_elements".into();
        store.save(&mut plan, 1).unwrap();
        let element_id = store
            .add_element(
                &plan.id,
                "external.issue",
                json!({
                    "title": "Fix launch",
                    "body": "Investigate the failing launch path."
                }),
                vec!["task_1".into()],
                Some(10.0),
                Some(20.0),
                2,
            )
            .unwrap();
        store
            .add_element(
                &plan.id,
                "note",
                json!({ "text": "Follow-up note\n\nDetails" }),
                Vec::new(),
                None,
                None,
                3,
            )
            .unwrap();
        (dir, store, plan.id, element_id)
    }

    #[test]
    fn list_elements_returns_generic_summaries() {
        let (_dir, store, plan_id, _element_id) = store_with_elements();

        let list = store
            .list_elements(&plan_id, Some("external.issue"))
            .unwrap();

        assert_eq!(list.elements.len(), 1);
        assert_eq!(list.elements[0].kind, "external.issue");
        assert_eq!(list.elements[0].title, "Fix launch");
        assert_eq!(list.elements[0].links, vec!["task_1"]);
        assert_eq!(list.elements[0].canvas.x, Some(10.0));
    }

    #[test]
    fn show_element_returns_raw_content_and_markdown() {
        let (_dir, store, plan_id, element_id) = store_with_elements();

        let details = store.show_element(&plan_id, &element_id).unwrap();

        assert_eq!(details.title, "Fix launch");
        assert_eq!(
            details.element.content["body"],
            "Investigate the failing launch path."
        );
        assert_eq!(details.markdown, "Investigate the failing launch path.");
    }

    #[test]
    fn title_falls_back_to_first_text_line() {
        let element = CanvasElement {
            id: "el_note".into(),
            kind: "note".into(),
            canvas: ElementCanvas::default(),
            content: json!({ "text": "\nFirst line\nSecond line" }),
            links: Vec::new(),
        };

        assert_eq!(element_title(&element), "First line");
    }
}
