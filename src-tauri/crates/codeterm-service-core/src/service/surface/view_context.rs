use crate::multiplexer::state::MultiplexerState;
use crate::service::ServiceContext;
use codeterm_plugin::view_host::ViewCallContext;

pub(crate) fn resolve_context(
    mux: &MultiplexerState,
    plugin: &str,
    surface: Option<&str>,
    instance: Option<&str>,
    tab: Option<&str>,
) -> Result<ViewCallContext, String> {
    let mut context = ViewCallContext::panel(plugin);
    match (surface, instance, tab) {
        (None, None, None) => return Ok(context),
        (Some(surface), Some(instance), Some(tab_id)) => {
            if surface.is_empty() || instance.is_empty() || tab_id.is_empty() {
                return Err("plugin_view_scope_invalid".into());
            }
            let tab = mux.get_tab(tab_id).ok_or("plugin_view_scope_invalid")?;
            let backing = tab.surface().ok_or("plugin_view_scope_invalid")?;
            if backing.plugin_id != plugin
                || backing.surface_id != surface
                || backing.instance_id != instance
            {
                return Err("plugin_view_scope_invalid".into());
            }
            let owner = tab.parent_tab_id.clone();
            if owner
                .as_ref()
                .is_some_and(|owner| mux.get_tab(owner).is_none())
            {
                return Err("plugin_view_scope_invalid".into());
            }
            context.workspace = mux.find_workspace_for_tab(tab_id);
            if let Some(owner_id) = &owner {
                if mux.find_workspace_for_tab(owner_id) != context.workspace {
                    return Err("plugin_view_scope_invalid".into());
                }
                context.team = mux
                    .get_tab(owner_id)
                    .and_then(|owner| owner.team_id.clone());
            }
            context.surface = Some(surface.into());
            context.instance = Some(instance.into());
            context.tab = Some(tab_id.into());
            context.owner = owner;
        }
        _ => return Err("plugin_view_scope_invalid".into()),
    }
    Ok(context)
}

impl ServiceContext {
    pub fn plugin_view_context(
        &self,
        plugin: &str,
        surface: Option<&str>,
        instance: Option<&str>,
        tab: Option<&str>,
    ) -> Result<ViewCallContext, String> {
        let context = resolve_context(&self.mux.read(), plugin, surface, instance, tab)?;
        if let Some(surface) = &context.surface {
            let declared = self
                .ext::<codeterm_plugin::surfaces::SurfaceRegistryHandle>()
                .get(plugin)
                .ok_or("plugin_view_scope_invalid")?;
            if !declared.iter().any(|item| {
                item.id == *surface
                    && item.renderer == codeterm_plugin::surfaces::SurfaceRenderer::View
            }) {
                return Err("plugin_view_scope_invalid".into());
            }
        }
        Ok(context)
    }

    /// Lease/provider lifetime is checked by the frame registry separately, before and after awaits.
    pub fn validate_plugin_view_context(&self, context: &ViewCallContext) -> Result<(), String> {
        if context.origin != codeterm_plugin::view_host::ViewCallOrigin::Document {
            return Err("plugin_view_scope_invalid".into());
        }
        let current = self.plugin_view_context(
            &context.plugin,
            context.surface.as_deref(),
            context.instance.as_deref(),
            context.tab.as_deref(),
        )?;
        if context.frame.is_some() {
            let captured = codeterm_plugin::media_delivery::context_provider(context)
                .map_err(|_| "plugin_view_frame_invalid")?;
            let provider = self
                .ext::<codeterm_plugin::view_host::ViewRegistryHandle>()
                .get(&context.plugin)
                .ok_or("plugin_view_frame_invalid")?;
            if !std::sync::Arc::ptr_eq(&captured, &provider) {
                return Err("plugin_view_frame_invalid".into());
            }
        }
        if current.owner != context.owner
            || current.workspace != context.workspace
            || current.team != context.team
        {
            return Err("plugin_view_scope_invalid".into());
        }
        Ok(())
    }
    /// Host-only synchronous worker boundary for a view-scoped binding; no JS-provided scope setter.
    pub fn with_validated_plugin_view_context<T>(
        &self,
        plugin: &str,
        context: &ViewCallContext,
        work: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        if context.plugin != plugin || context.frame.is_none() {
            return Err("plugin_view_frame_invalid".into());
        }
        self.validate_plugin_view_context(context)?;
        let result = work();
        self.validate_plugin_view_context(context)?;
        result
    }

    /// Internal monitor port: the persisted host scope selects live views, never callback target words.
    pub fn invalidate_plugin_owner_views(
        &self,
        scope: &codeterm_plugin::view_host::ViewOwnerScope,
    ) -> Result<usize, String> {
        let declarations = self.ext::<codeterm_plugin::surfaces::SurfaceRegistryHandle>();
        let Some(declared) = declarations.get(&scope.plugin) else {
            return Ok(0);
        };
        let mut seen = std::collections::HashSet::new();
        let mut count = 0;
        for context in codeterm_plugin::media_delivery::active_view_contexts(&scope.plugin) {
            if context.owner_scope().as_ref() != Some(scope)
                || self.validate_plugin_view_context(&context).is_err()
            {
                continue;
            }
            let (Some(surface), Some(instance), Some(tab)) =
                (context.surface, context.instance, context.tab)
            else {
                continue;
            };
            if !declared.iter().any(|decl| {
                decl.id == surface
                    && decl.state_invalidation
                    && decl.renderer == codeterm_plugin::surfaces::SurfaceRenderer::View
            }) || !seen.insert(tab.clone())
            {
                continue;
            }
            self.invalidate_surface_view(
                &scope.plugin,
                &serde_json::json!({"surface":surface,"instance":instance,"tab":tab}).to_string(),
            )?;
            count += 1;
        }
        Ok(count)
    }
}

#[cfg(test)]
mod tests;
