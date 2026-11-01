//! Step calls never resolve an owner's existing surface. Open creates a separate surface;
//! subsequent calls require its exact tab and instance in this host-stamped submit scope.
use super::{
    agent_call::{self, CallPlan},
    model::SurfaceOpenRequest,
};
use crate::service::ServiceContext;
use codeterm_plugin::{blueprint_step::StepInputContext, surfaces::SurfaceRegistryHandle};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Identity {
    tab: String,
    instance: String,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct Call {
    #[serde(default)]
    args: Vec<String>,
    surface: Option<Identity>,
}

#[cfg(test)]
mod tests;

fn parse(raw: &str) -> Result<Call, String> {
    if raw.len() > 32 * 1024 {
        return Err("surface arguments too large".into());
    }
    let value: Value = serde_json::from_str(raw).map_err(|_| "invalid step surface call")?;
    if value.is_array() {
        return Ok(Call {
            args: serde_json::from_value(value).map_err(|_| "invalid surface args")?,
            surface: None,
        });
    }
    serde_json::from_value(value).map_err(|_| "invalid step surface call".into())
}

impl ServiceContext {
    pub(crate) fn step_surface_call(
        &self,
        plugin: &str,
        context: &StepInputContext,
        verb: &str,
        raw: &str,
    ) -> String {
        match self.step_surface_call_inner(plugin, context, verb, raw) {
            Ok(value) => value.to_string(),
            Err(error) => json!({"error":error}).to_string(),
        }
    }

    fn step_surface_call_inner(
        &self,
        plugin: &str,
        context: &StepInputContext,
        verb: &str,
        raw: &str,
    ) -> Result<Value, String> {
        if context.plugin != plugin || !context.submitting {
            return Err("step surface scope required".into());
        }
        let call = parse(raw)?;
        let declared = self
            .ext::<SurfaceRegistryHandle>()
            .get(plugin)
            .ok_or("surface plugin unavailable")?;
        let surface =
            agent_call::plugin_agent_surface(&declared).ok_or("no agent-callable surface")?;
        if surface.renderer == codeterm_plugin::surfaces::SurfaceRenderer::View
            && !matches!(verb, "open" | "close" | "status")
            && !surface.agent_verbs.iter().any(|declared| declared == verb)
        {
            return Err("surface verb is not declared".into());
        }
        let owner = &context.request.context.owner;
        let caller = {
            let mux = self.mux.read();
            agent_call::resolve_caller_tab(&mux, owner).ok_or("step owner closed")?
        };
        if verb == "status" && call.surface.is_none() {
            return Ok(
                json!({"result": if context.surfaces.lock().is_empty() { format!("no {} open; run `open` first", surface.title.to_lowercase()) } else { "step surface identity required".to_string() }}),
            );
        }
        if verb == "open" {
            if call.surface.is_some() || !context.surfaces.lock().is_empty() {
                return Err("step already opened a surface".into());
            }
            let opened = self
                .open_surface(SurfaceOpenRequest {
                    target: format!("{plugin}:{}", surface.id),
                    owner_tab_id: Some(caller.clone()),
                    caller_tab_id: Some(caller.clone()),
                    ..Default::default()
                })
                .map_err(|error| error.message())?;
            context
                .surfaces
                .lock()
                .insert(opened.tab_id.clone(), opened.instance_id.clone());
            let result = self.execute_surface_plan(
                plugin,
                surface,
                caller.clone(),
                verb,
                call.args,
                CallPlan::Run {
                    tab_id: opened.tab_id.clone(),
                    actor: None,
                },
            );
            match result {
                Ok(mut value) => {
                    value["surface"] = json!({"tab":opened.tab_id,"instance":opened.instance_id});
                    return Ok(value);
                }
                Err(error) => {
                    let _ = self.close_surface(Some(&caller), &opened.tab_id);
                    context.surfaces.lock().remove(&opened.tab_id);
                    return Err(error);
                }
            }
        }
        let identity = call.surface.ok_or("step surface identity required")?;
        {
            let mux = self.mux.read();
            if context.surfaces.lock().get(&identity.tab) != Some(&identity.instance) {
                return Err("surface was not opened by this step run".into());
            }
            super::view::validate_instance(
                &mux,
                plugin,
                &super::view::ViewIdentity {
                    surface: surface.id.clone(),
                    tab: identity.tab.clone(),
                    instance: identity.instance,
                },
            )?;
            if mux
                .get_tab(&identity.tab)
                .and_then(|tab| tab.parent_tab_id.as_deref())
                != Some(caller.as_str())
            {
                return Err("step surface owner changed".into());
            }
        }
        let plan = if verb == "close" {
            CallPlan::Close {
                tab_id: identity.tab.clone(),
            }
        } else {
            CallPlan::Run {
                tab_id: identity.tab.clone(),
                actor: None,
            }
        };
        let result = self.execute_surface_plan(plugin, surface, caller, verb, call.args, plan);
        if verb == "close" && result.is_ok() {
            context.surfaces.lock().remove(&identity.tab);
        }
        result
    }
}
