use codeterm_plugin::surfaces::SurfaceDecl;
use serde_json::{json, Value};

use super::model::SurfaceOpenRequest;
use crate::multiplexer::state::MultiplexerState;
use crate::service::ServiceContext;

/// `host.surface.call(verb, argsJson)` → `{ result }` or `{ error }`, as one JSON string.
pub(crate) fn reply(outcome: Result<String, String>) -> String {
    match outcome {
        Ok(result) => json!({ "result": result }).to_string(),
        Err(error) => json!({ "error": error }).to_string(),
    }
}

/// `argsJson` is a JSON array of words; non-string items keep their JSON text.
pub(crate) fn parse_verb_args(args_json: &str) -> Result<Vec<String>, String> {
    if args_json.trim().is_empty() {
        return Ok(Vec::new());
    }
    match serde_json::from_str::<Value>(args_json) {
        Ok(Value::Array(items)) => Ok(items
            .into_iter()
            .map(|item| match item {
                Value::String(text) => text,
                other => other.to_string(),
            })
            .collect()),
        Ok(Value::Null) => Ok(Vec::new()),
        _ => Err("host.surface.call args must be a JSON array of strings".into()),
    }
}

/// A caller id is a tab id or the worker id the CLI sends when it runs inside a worker.
pub(crate) fn resolve_caller_tab(mux: &MultiplexerState, caller: &str) -> Option<String> {
    let caller = caller.trim();
    if caller.is_empty() {
        return None;
    }
    mux.resolve_tab_id(caller).or_else(|| {
        mux.all_tabs()
            .find(|tab| tab.worker_id.as_deref() == Some(caller))
            .map(|tab| tab.id.clone())
    })
}

/// The first stream surface a plugin declares: the one its agent verbs act on.
pub(crate) fn plugin_stream_surface(declared: &[SurfaceDecl]) -> Option<&SurfaceDecl> {
    declared
        .iter()
        .find(|surface| surface.stream_runtime().is_some())
}

/// The caller's own open surface of that declaration, if any.
pub(crate) fn caller_surface_tab(
    mux: &MultiplexerState,
    plugin_id: &str,
    surface_id: &str,
    caller_tab_id: &str,
) -> Option<String> {
    mux.all_tabs()
        .filter(|tab| tab.parent_tab_id.as_deref() == Some(caller_tab_id))
        .find(|tab| {
            tab.surface().is_some_and(|surface| {
                surface.plugin_id == plugin_id && surface.surface_id == surface_id
            })
        })
        .map(|tab| tab.id.clone())
}

impl ServiceContext {
    /// Runs on the plugin's calling thread and blocks it; the plugin VM is held only by that plugin.
    pub(crate) fn surface_agent_call_json(
        &self,
        plugin_id: &str,
        caller: Option<&str>,
        verb: &str,
        args_json: &str,
    ) -> String {
        reply(self.surface_agent_call(plugin_id, caller, verb, args_json))
    }

    fn surface_agent_call(
        &self,
        plugin_id: &str,
        caller: Option<&str>,
        verb: &str,
        args_json: &str,
    ) -> Result<String, String> {
        let args = parse_verb_args(args_json)?;
        let declared = self
            .surface_registry
            .get(plugin_id)
            .map(|declared| declared.to_vec())
            .unwrap_or_default();
        let surface = plugin_stream_surface(&declared)
            .ok_or_else(|| format!("plugin {plugin_id} declares no stream surface"))?;
        if verb == "help" {
            return Ok(self.surface_agent_guide(plugin_id));
        }
        let caller_tab = {
            let mux = self.mux.read();
            caller.and_then(|caller| resolve_caller_tab(&mux, caller))
        }
        .ok_or("host.surface.call acts for the calling agent; run it from an agent Tab")?;
        let open_tab = {
            let mux = self.mux.read();
            caller_surface_tab(&mux, plugin_id, &surface.id, &caller_tab)
        };
        let noun = surface.title.to_lowercase();
        let tab_id = match (open_tab, verb) {
            (Some(tab_id), _) => tab_id,
            (None, "open") => {
                self.open_surface(SurfaceOpenRequest {
                    target: format!("{plugin_id}:{}", surface.id),
                    owner_tab_id: Some(caller_tab.clone()),
                    caller_tab_id: Some(caller_tab.clone()),
                    ..Default::default()
                })
                .map_err(|error| error.message())?
                .tab_id
            }
            (None, "status") => return Ok(format!("no {noun} open; run `open` first")),
            (None, "close") => return Ok(format!("no {noun} open")),
            (None, _) => {
                return Err(format!(
                    "no {noun} open for this agent; run `codeterm plugin {plugin_id} open` first"
                ))
            }
        };
        if verb == "close" {
            self.close_surface(Some(caller_tab.as_str()), &tab_id)
                .map_err(|error| error.message())?;
            return Ok(format!("closed {noun} {tab_id}"));
        }
        let (runtime, binding) = self
            .surface_runtime(&tab_id)
            .map_err(|error| error.message())?;
        let ctx = self.surface_call_ctx(&binding);
        self.note_surface_activity(&binding.tab_id, verb);
        let verb = verb.to_string();
        block_on_runtime(async move { runtime.agent_verb(&ctx, &verb, &args).await })
    }

    fn surface_agent_guide(&self, plugin_id: &str) -> String {
        let catalog = self
            .agent_command_registry
            .get(plugin_id)
            .and_then(|handler| handler.catalog());
        match catalog {
            Some(catalog) => catalog
                .guide
                .clone()
                .unwrap_or_else(|| format!("Verbs: {}", catalog.verbs.join(", "))),
            None => format!("plugin {plugin_id} has no agent guide"),
        }
    }
}

/// Plugin calls arrive on socket threads or the blocking pool; never block an async worker in place.
fn block_on_runtime<F: std::future::Future>(future: F) -> F::Output {
    use tokio::runtime::{Handle, RuntimeFlavor};
    match Handle::try_current() {
        Ok(handle) if handle.runtime_flavor() == RuntimeFlavor::MultiThread => {
            tokio::task::block_in_place(|| handle.block_on(future))
        }
        _ => tauri::async_runtime::block_on(future),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::multiplexer::types::{SurfaceBacking, TabBacking};

    #[test]
    fn surface_agent_call_reply_is_result_or_error_json() {
        let ok: Value = serde_json::from_str(&reply(Ok("done".into()))).unwrap();
        assert_eq!(ok, json!({ "result": "done" }));
        let err: Value = serde_json::from_str(&reply(Err("no".into()))).unwrap();
        assert_eq!(err, json!({ "error": "no" }));
    }

    #[test]
    fn surface_agent_call_args_are_a_word_array() {
        assert_eq!(
            parse_verb_args(r#"["button","Sign in"]"#).unwrap(),
            vec!["button".to_string(), "Sign in".to_string()]
        );
        assert_eq!(
            parse_verb_args(r#"[800]"#).unwrap(),
            vec!["800".to_string()]
        );
        assert!(parse_verb_args("").unwrap().is_empty());
        assert!(parse_verb_args("null").unwrap().is_empty());
        assert!(parse_verb_args(r#"{"a":1}"#).is_err());
    }

    #[test]
    fn surface_agent_call_resolves_the_caller_by_tab_or_worker_id_and_finds_its_own_surface() {
        let (mut mux, owner) = MultiplexerState::new();
        let workspace = mux.list_workspaces()[0].id.clone();
        mux.get_tab_mut(&owner).unwrap().worker_id = Some("w-7".into());
        assert_eq!(resolve_caller_tab(&mux, &owner), Some(owner.clone()));
        assert_eq!(resolve_caller_tab(&mux, "w-7"), Some(owner.clone()));
        assert_eq!(resolve_caller_tab(&mux, "nobody"), None);
        assert_eq!(resolve_caller_tab(&mux, " "), None);

        let other = mux.create_tab(&workspace).unwrap();
        for (parent, surface_id) in [(&other, "browser"), (&owner, "board"), (&owner, "browser")] {
            let tab = mux.create_tab(&workspace).unwrap();
            let surface = mux.get_tab_mut(&tab).unwrap();
            surface.backing = TabBacking::Surface(SurfaceBacking {
                plugin_id: "web".into(),
                surface_id: surface_id.into(),
                instance_id: tab.clone(),
                owner_label: true,
                resume_url: None,
                resume_profile: None,
            });
            surface.parent_tab_id = Some(parent.clone());
        }
        let found = caller_surface_tab(&mux, "web", "browser", &owner).expect("own surface");
        let tab = mux.get_tab(&found).unwrap();
        assert_eq!(tab.parent_tab_id.as_deref(), Some(owner.as_str()));
        assert_eq!(tab.surface().unwrap().surface_id, "browser");
        assert!(caller_surface_tab(&mux, "other", "browser", &owner).is_none());
    }

    #[test]
    fn surface_agent_call_targets_the_first_stream_declaration() {
        let view: SurfaceDecl =
            serde_json::from_value(json!({ "id": "board", "title": "Board", "renderer": "view" }))
                .unwrap();
        let stream: SurfaceDecl = serde_json::from_value(
            json!({ "id": "browser", "title": "Browser", "renderer": "stream", "runtime": "browser" }),
        )
        .unwrap();
        assert_eq!(
            plugin_stream_surface(&[view.clone(), stream]).map(|s| s.id.as_str()),
            Some("browser")
        );
        assert!(plugin_stream_surface(&[view]).is_none());
    }
}
