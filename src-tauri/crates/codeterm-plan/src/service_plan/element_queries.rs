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
        let kind = kind.map(str::trim).filter(|value| !value.is_empty());
        let elements = self.database.element_summaries(plan_id, kind)?;
        Ok(ElementList {
            plan_id: plan_id.to_string(),
            elements,
        })
    }

    pub fn show_element(
        &self,
        plan_id: &str,
        element_id: &str,
    ) -> Result<ElementDetails, PlanError> {
        let element = self.database.selected_element(plan_id, element_id)?;
        let title = element_title(&element);
        let markdown = element_markdown(&element);
        Ok(ElementDetails {
            plan_id: plan_id.to_string(),
            element,
            title,
            markdown,
        })
    }
}

#[cfg(test)]
pub(super) fn element_summary(element: &CanvasElement) -> ElementSummary {
    ElementSummary {
        id: element.id.clone(),
        kind: element.kind.clone(),
        title: element_title(element),
        links: element.links.clone(),
        canvas: element.canvas.clone(),
    }
}

fn element_title(element: &CanvasElement) -> String {
    element_title_fields(
        &element.id,
        ["title", "name", "label", "text", "body", "markdown"]
            .map(|key| element.content.get(key).and_then(Value::as_str)),
    )
}

pub(super) fn element_title_fields(id: &str, fields: [Option<&str>; 6]) -> String {
    for (index, value) in fields.into_iter().enumerate() {
        let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
            continue;
        };
        if index < 3 {
            return value.to_string();
        }
        if let Some(line) = value.lines().map(str::trim).find(|line| !line.is_empty()) {
            return line.to_string();
        }
    }
    id.to_string()
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

#[cfg(test)]
mod tests;
