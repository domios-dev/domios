use serde::Serialize;
use std::cell::RefCell;

/// UI-origin actor. An iframe cannot claim an agent's identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ViewActor {
    Local,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewCallOrigin {
    Document,
    RemoteTask,
    RecordingSetup,
}

/// Resolved by a host document or narrow host port; never deserialize from iframe args.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ViewCallContext {
    pub plugin: String,
    pub origin: ViewCallOrigin,
    pub surface: Option<String>,
    pub instance: Option<String>,
    pub tab: Option<String>,
    pub owner: Option<String>,
    pub workspace: Option<String>,
    pub team: Option<String>,
    pub actor: ViewActor,
    /// Non-secret document identity, distinct from the private authorization lease.
    pub frame: Option<String>,
}

impl ViewCallContext {
    pub fn panel(plugin: impl Into<String>) -> Self {
        Self {
            plugin: plugin.into(),
            origin: ViewCallOrigin::Document,
            surface: None,
            instance: None,
            tab: None,
            owner: None,
            workspace: None,
            team: None,
            actor: ViewActor::Local,
            frame: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewOwnerScope {
    pub plugin: String,
    pub workspace: String,
    pub team: Option<String>,
    pub owner: String,
}

impl ViewCallContext {
    pub fn owner_scope(&self) -> Option<ViewOwnerScope> {
        Some(ViewOwnerScope {
            plugin: self.plugin.clone(),
            workspace: self.workspace.clone()?,
            team: self.team.clone(),
            owner: self.owner.clone()?,
        })
    }
}

thread_local! {
    static INVOCATION: RefCell<Option<ViewCallContext>> = const { RefCell::new(None) };
}

/// Rust-only host projection. Consumers must revalidate the live registration at admission.
pub fn current_view_call_context(plugin: &str) -> Option<ViewCallContext> {
    INVOCATION.with(|value| {
        value
            .borrow()
            .as_ref()
            .filter(|context| context.plugin == plugin)
            .cloned()
    })
}

pub fn with_view_call_context<T>(
    plugin: &str,
    context: &ViewCallContext,
    work: impl FnOnce() -> Result<T, super::ViewError>,
) -> Result<T, super::ViewError> {
    if context.plugin != plugin {
        return Err(super::ViewError::Call("plugin_view_scope_invalid".into()));
    }
    let _scope = ViewInvocationScope::enter(context.clone());
    work()
}

pub(crate) struct ViewInvocationScope(Option<ViewCallContext>);
impl ViewInvocationScope {
    pub(crate) fn enter(context: ViewCallContext) -> Self {
        Self(INVOCATION.with(|value| value.replace(Some(context))))
    }
}
impl Drop for ViewInvocationScope {
    fn drop(&mut self) {
        INVOCATION.with(|value| *value.borrow_mut() = self.0.take());
    }
}

#[cfg(test)]
mod tests;
