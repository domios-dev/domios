use codeterm_plugin::surfaces::{SurfaceRegistryHandle, SurfaceRenderer};
use serde_json::{json, Value};

use crate::multiplexer::state::MultiplexerState;
use crate::service::ServiceContext;

/// No plugin-supplied owner, actor, revision or arbitrary event data crosses this port.
pub(crate) use codeterm_wire::plugin_host::ViewIdentity;

pub(crate) fn view_context(
    mux: &MultiplexerState,
    plugin: &str,
    surface: &str,
    tab_id: &str,
    actor: &str,
) -> Result<Value, String> {
    let tab = mux.get_tab(tab_id).ok_or("surface instance is closed")?;
    let backing = tab.surface().ok_or("tab is not a surface")?;
    if backing.plugin_id != plugin
        || backing.surface_id != surface
        || backing.instance_id.is_empty()
    {
        return Err("surface identity mismatch".into());
    }
    let owner = tab
        .parent_tab_id
        .as_deref()
        .filter(|id| mux.get_tab(id).is_some())
        .ok_or("surface owner is closed")?;
    let actor_label = mux
        .get_tab(actor)
        .and_then(|tab| super::activity::sanitize_actor(&tab.title))
        .unwrap_or_else(|| "agent".to_string());
    Ok(json!(codeterm_wire::plugin_host::SurfaceCallContext {
        actor_label,
        surface,
        instance: &backing.instance_id,
        tab: tab_id,
        owner,
        actor
    }))
}

pub(crate) fn validate_instance(
    mux: &MultiplexerState,
    plugin: &str,
    identity: &ViewIdentity,
) -> Result<(), String> {
    let tab = mux
        .get_tab(&identity.tab)
        .ok_or("surface instance is closed")?;
    let backing = tab.surface().ok_or("tab is not a surface")?;
    if backing.plugin_id != plugin || backing.surface_id != identity.surface {
        return Err("surface identity mismatch".into());
    }
    if backing.instance_id != identity.instance || identity.instance.is_empty() {
        return Err("surface instance is stale".into());
    }
    Ok(())
}

impl ServiceContext {
    pub(crate) fn invalidate_surface_view_json(&self, plugin: &str, request: &str) -> String {
        match self.invalidate_surface_view(plugin, request) {
            Ok(()) => serde_json::to_string(&codeterm_wire::plugin_host::HostOk { ok: true })
                .unwrap_or_else(|_| "null".into()),
            Err(error) => serde_json::to_string(&codeterm_wire::plugin_host::HostError { error })
                .unwrap_or_else(|_| "null".into()),
        }
    }

    pub(super) fn invalidate_surface_view(
        &self,
        plugin: &str,
        request: &str,
    ) -> Result<(), String> {
        if request.len() > 1024 {
            return Err("surface invalidation exceeds 1024 bytes".into());
        }
        let identity: ViewIdentity =
            serde_json::from_str(request).map_err(|_| "invalid surface identity")?;
        let declarations = self.ext::<SurfaceRegistryHandle>();
        let declared = declarations
            .get(plugin)
            .ok_or("surface plugin is unavailable")?;
        if !declared.iter().any(|decl| {
            decl.id == identity.surface
                && decl.renderer == SurfaceRenderer::View
                && decl.state_invalidation
        }) {
            return Err("surface state invalidation is not declared".into());
        }
        {
            let mux = self.mux.read();
            validate_instance(&mux, plugin, &identity)?;
        }
        let payload = json!({"plugin":plugin,"surface":identity.surface,"instance":identity.instance,"tab":identity.tab});
        self.events
            .publish(codeterm_events::SystemEvent::SurfaceStateInvalidated {
                plugin: plugin.to_string(),
                surface: identity.surface,
                instance: identity.instance,
                tab: identity.tab,
            });
        self.ui.emit_native("surface-state-invalidated", payload);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
