//! Blueprint automation for Team template payloads. Start imports a package's Blueprint
//! stopped, in the Team scope, under the Team's Orchestrator; bake exports the Team's
//! Blueprints as portable v2 documents. Both go through the ordinary Blueprint authority.

use std::sync::{Arc, Weak};

use codeterm_blueprint_types::blueprint_document::{self, Document};
use codeterm_blueprint_types::blueprint_recipe::{Recipe, SCHEMA_VERSION};
use codeterm_service_core::service::team_template_payload::{
    install_automation_port, live_orchestrator, AutomationItem, AutomationPort,
};
use codeterm_service_core::service::ServiceContext;
use serde_json::Value;

use crate::scheduler::Handle;
use crate::BlueprintPortableExt;

struct TeamAutomation {
    svc: Weak<ServiceContext>,
}

/// A package Blueprint as an import recipe; invalid documents are refused with their diagnostics.
pub fn recipe(item: &AutomationItem) -> Result<Recipe, String> {
    let parsed = blueprint_document::parse(&item.name, &item.content).map_err(|diagnostics| {
        diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.clone())
            .collect::<Vec<_>>()
            .join("; ")
    })?;
    let recipe = Recipe {
        schema_version: SCHEMA_VERSION,
        definition: serde_json::to_value(&parsed.document).map_err(|error| error.to_string())?,
    };
    recipe.validate()?;
    Ok(recipe)
}

fn recipe_name(recipe: &Recipe) -> Option<String> {
    recipe
        .definition
        .get("name")
        .and_then(Value::as_str)
        .map(str::to_string)
}

impl TeamAutomation {
    fn service(&self) -> Result<Arc<ServiceContext>, String> {
        self.svc
            .upgrade()
            .ok_or_else(|| "service is shutting down".to_string())
    }
}

impl AutomationPort for TeamAutomation {
    fn check(&self, item: &AutomationItem) -> Result<(), String> {
        recipe(item).map(|_| ())
    }

    fn apply(&self, team_id: &str, item: &AutomationItem) -> Result<(), String> {
        let svc = self.service()?;
        let recipe = recipe(item)?;
        let name = recipe_name(&recipe).unwrap_or_else(|| item.name.clone());
        if self
            .list(team_id)?
            .iter()
            .any(|existing| existing.name == name)
        {
            return Ok(());
        }
        let orchestrator = live_orchestrator(&svc, team_id)
            .ok_or("the Team's Orchestrator has not proved its launch yet")?;
        svc.blueprint_import_host(&orchestrator, recipe, &[], Some(&format!("team:{team_id}")))
            .map(|_| ())
    }

    fn list(&self, team_id: &str) -> Result<Vec<AutomationItem>, String> {
        let svc = self.service()?;
        let Some(handle) = svc.host_extension::<Handle>() else {
            return Ok(Vec::new());
        };
        let jobs: Vec<_> = {
            let store = handle.store.lock();
            store.ensure_available()?;
            store
                .jobs
                .iter()
                .filter(|job| job.team_id.as_deref() == Some(team_id))
                .cloned()
                .collect()
        };
        let mut items = Vec::new();
        for job in &jobs {
            let recipe = match crate::portable::export(job) {
                Ok(recipe) => recipe,
                Err(error) => {
                    log::debug!(
                        "[team-templates] Blueprint {} stays behind: {error}",
                        job.id
                    );
                    continue;
                }
            };
            items.push(AutomationItem {
                name: recipe_name(&recipe).unwrap_or_else(|| job.id.clone()),
                description: None,
                content: Document::from_recipe(&recipe)?.yaml()?,
            });
        }
        items.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(items)
    }
}

/// Installs the Team template automation port; the daemon calls it once Blueprints run.
pub fn install(svc: &Arc<ServiceContext>) {
    install_automation_port(Arc::new(TeamAutomation {
        svc: Arc::downgrade(svc),
    }));
}

#[cfg(test)]
#[path = "team_automation_tests.rs"]
mod tests;
