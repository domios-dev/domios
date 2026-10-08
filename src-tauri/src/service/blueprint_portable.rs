use codeterm_types::blueprint_recipe::Recipe;
use serde_json::Value;
use tauri::Manager;

use super::{sentinel::SentinelCaller, ServiceContext};
use crate::sentinel::{portable, scheduler::Handle};

impl ServiceContext {
    pub(crate) fn blueprint_import_host(
        &self,
        tab_id: &str,
        recipe: Recipe,
        assignments: &[String],
    ) -> Result<Value, String> {
        let nonce = {
            let mux = self.mux.read();
            mux.get_tab(tab_id)
                .ok_or("Blueprint creator tab not found")?
                .session_binding_nonce
                .clone()
        };
        let caller = self.sentinel_caller(tab_id, nonce.as_deref())?;
        self.blueprint_import(&caller, recipe, assignments, true)
    }

    pub(crate) fn blueprint_export(
        &self,
        caller: &SentinelCaller,
        id: &str,
    ) -> Result<Recipe, String> {
        portable::export(&self.sentinel_show(caller, id)?)
    }

    pub(crate) fn blueprint_import(
        &self,
        caller: &SentinelCaller,
        recipe: Recipe,
        assignments: &[String],
        stopped: bool,
    ) -> Result<Value, String> {
        if caller.team.is_none() {
            return Err("Blueprint import requires a caller in a Team".into());
        }
        let mut definition = recipe.bind(assignments)?;
        let fields = definition
            .as_object_mut()
            .ok_or("recipe definition must be an object")?;
        if fields.len() != 1 {
            return Err("recipe definition must contain exactly one of create or spec".into());
        }
        if let Some(create) = fields.remove("create") {
            let create = crate::sentinel::public_definition::stored_definition(create)?;
            serde_json::to_value(self.sentinel_create_stopped(
                caller,
                serde_json::from_value(create).map_err(|error| error.to_string())?,
                stopped,
            )?)
            .map_err(|error| error.to_string())
        } else if let Some(spec) = fields.remove("spec") {
            self.blueprint_pipeline_create_stopped(
                caller,
                serde_json::from_value(spec).map_err(|error| error.to_string())?,
                stopped,
            )
        } else {
            Err("recipe definition must contain create or spec".into())
        }
    }

    pub(crate) fn blueprint_export_host(
        &self,
        workspace: &str,
        team: Option<&str>,
        id: &str,
    ) -> Result<Recipe, String> {
        {
            let mux = self.mux.read();
            crate::sentinel::graph::validate_scope(&mux, workspace, team)?;
        }
        let handle = self
            .app_handle
            .try_state::<Handle>()
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
