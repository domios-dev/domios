use super::agent_call::is_agent_tab;
use crate::service::ServiceContext;
use codeterm_plugin::{
    surface_invocation::SurfaceInvocationContext,
    surfaces::{SurfaceRegistryHandle, SurfaceRenderer},
    view_host::ViewRegistryHandle,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

impl ServiceContext {
    pub fn plugin_agent_invocation(
        &self,
        plugin: &str,
        surface: &str,
        caller: &str,
    ) -> Result<SurfaceInvocationContext, String> {
        let views = self.ext::<ViewRegistryHandle>();
        let (provider, registration) = views.registered(plugin).ok_or("plugin unavailable")?;
        if provider
            .job_lifecycle()
            .is_none_or(|capability| !capability.valid())
        {
            return Err("plugin job lifecycle unavailable".into());
        }
        let declarations = self
            .ext::<SurfaceRegistryHandle>()
            .get(plugin)
            .ok_or("plugin surface unavailable")?;
        if !declarations.iter().any(|declaration| {
            declaration.id == surface
                && declaration.renderer == SurfaceRenderer::View
                && !declaration.agent_verbs.is_empty()
        }) {
            return Err("agent-callable view required".into());
        }
        let mux = self.mux.read();
        let tab = mux
            .get_tab(caller)
            .filter(|tab| is_agent_tab(tab))
            .ok_or("live calling agent required")?;
        let workspace = mux
            .find_workspace_for_tab(caller)
            .ok_or("agent workspace unavailable")?;
        let identity = json!([
            plugin,
            registration,
            surface,
            tab.id,
            workspace,
            tab.team_id,
            tab.agent_session_id,
            tab.session_binding_nonce
        ]);
        let instance = format!("{:x}", Sha256::digest(identity.to_string().as_bytes()));
        Ok(SurfaceInvocationContext {
            plugin: plugin.into(),
            registration,
            surface: surface.into(),
            instance,
            tab: tab.id.clone(),
            owner: tab.id.clone(),
            actor: tab.id.clone(),
            workspace,
            team: tab.team_id.clone(),
        })
    }

    pub(super) fn headless_surface_dispatch(
        &self,
        plugin: &str,
        surface: &str,
        caller: &str,
        verb: &str,
        args: &[String],
    ) -> Result<Value, String> {
        let invocation = self.plugin_agent_invocation(plugin, surface, caller)?;
        let context = json!({"surface":surface,"instance":invocation.instance,"tab":caller,"owner":caller,"actor":caller,"actorLabel":"agent","workspace":invocation.workspace,"team":invocation.team,"headless":true,"jobContext":invocation.instance});
        let ticket = codeterm_plugin::surface_invocation::issue(invocation)?;
        Ok(json!({"view_dispatch":{"context":context,"verb":verb,"args":args,"invocation":ticket}}))
    }
}
