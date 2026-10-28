use codeterm_blueprint_types::blueprint_recipe::Recipe;
use serde_json::Value;

use crate::service_adapter::{SentinelCaller, SentinelExt};
use crate::BlueprintApplyExt;
use crate::{portable, scheduler::Handle};
use codeterm_service_core::service::ServiceContext;

pub trait BlueprintPortableExt {
    fn blueprint_import_host(
        &self,
        tab_id: &str,
        recipe: Recipe,
        assignments: &[String],
        scope: Option<&str>,
    ) -> Result<Value, String>;
    fn blueprint_import_destination(
        &self,
        caller: &SentinelCaller,
        scope: Option<&str>,
    ) -> Result<(), String>;
    fn blueprint_export(&self, caller: &SentinelCaller, id: &str) -> Result<Recipe, String>;
    fn blueprint_import(
        &self,
        caller: &SentinelCaller,
        recipe: Recipe,
        assignments: &[String],
        stopped: bool,
    ) -> Result<Value, String>;
    fn blueprint_export_host(
        &self,
        workspace: &str,
        team: Option<&str>,
        id: &str,
    ) -> Result<Recipe, String>;
}

impl BlueprintPortableExt for ServiceContext {
    fn blueprint_import_host(
        &self,
        tab_id: &str,
        recipe: Recipe,
        assignments: &[String],
        scope: Option<&str>,
    ) -> Result<Value, String> {
        let nonce = {
            let mux = self.mux.read();
            mux.get_tab(tab_id)
                .ok_or("Blueprint creator tab not found")?
                .session_binding_nonce
                .clone()
        };
        let caller = self.sentinel_caller(tab_id, nonce.as_deref())?;
        self.blueprint_import_destination(&caller, scope)?;
        self.blueprint_import(&caller, recipe, assignments, true)
    }

    fn blueprint_import_destination(
        &self,
        caller: &SentinelCaller,
        scope: Option<&str>,
    ) -> Result<(), String> {
        let mux = self.mux.read();
        if !crate::service_adapter::caller_is_live(&mux, caller) {
            return Err("Blueprint caller scope changed".into());
        }
        let Some(scope) = scope else {
            return Ok(());
        };
        let matches = if scope == "general" {
            caller.team.is_none()
                && codeterm_service_core::service::general_agent::general_workspace_id(&mux)
                    .as_deref()
                    == Some(caller.workspace.as_str())
        } else if let Some(workspace) = scope.strip_prefix("workspace:") {
            !workspace.is_empty() && caller.team.is_none() && caller.workspace == workspace
        } else if let Some(team) = scope.strip_prefix("team:") {
            !team.is_empty() && caller.team.as_deref() == Some(team)
        } else {
            return Err("Blueprint scope must be general, workspace:<id>, or team:<id>".into());
        };
        if !matches {
            return Err(
                "Blueprint destination requires a caller belonging to that exact scope".into(),
            );
        }
        crate::graph::validate_scope(&mux, &caller.workspace, caller.team.as_deref())
    }

    fn blueprint_export(&self, caller: &SentinelCaller, id: &str) -> Result<Recipe, String> {
        portable::export(&self.sentinel_show(caller, id)?)
    }

    fn blueprint_import(
        &self,
        caller: &SentinelCaller,
        recipe: Recipe,
        assignments: &[String],
        stopped: bool,
    ) -> Result<Value, String> {
        self.blueprint_import_destination(caller, None)?;
        let definition = recipe.bind(assignments)?;
        let result = self
            .blueprint_apply_with_state(caller, definition, false, Some(stopped))
            .map_err(|error| error.to_string())?;
        let id = result["id"]
            .as_str()
            .ok_or("Applied Blueprint ID missing")?;
        let handle = self
            .host_extension::<Handle>()
            .ok_or("blueprints not initialized")?;
        let job = handle
            .store
            .lock()
            .get(id)
            .cloned()
            .ok_or("Applied Blueprint missing")?;
        let mut result = result;
        result["blueprint"] = serde_json::to_value(job).map_err(|error| error.to_string())?;
        Ok(result)
    }

    fn blueprint_export_host(
        &self,
        workspace: &str,
        team: Option<&str>,
        id: &str,
    ) -> Result<Recipe, String> {
        {
            let mux = self.mux.read();
            crate::graph::validate_scope(&mux, workspace, team)?;
        }
        let handle = self
            .host_extension::<Handle>()
            .ok_or("blueprints not initialized")?;
        let job = {
            let store = handle.store.lock();
            store.ensure_available()?;
            store
                .get(id)
                .filter(|job| job.workspace_id == workspace && job.team_id.as_deref() == team)
                .cloned()
                .ok_or("Blueprint not found in this scope")?
        };
        portable::export(&job)
    }
}

#[cfg(test)]
mod tests;
