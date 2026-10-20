use codeterm_plugin::surfaces::SurfaceDecl;
use serde_json::{json, Value};

use super::activity::sanitize_actor;
use super::model::SurfaceOpenRequest;
use super::runtime::{SurfaceCallOutcome, SurfaceEffect, SurfaceRuntime};
use super::stream_ops::StreamBinding;
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::Tab;
use crate::service::ServiceContext;

const ARGS_SHAPE: &str =
    "host.surface.call args must be a JSON array of strings or { \"args\": [...], \"of\": \"<agent>\" }";
const OF_USAGE: &str = "--of needs an agent name or tab id";

/// `host.surface.call(verb, argsJson)` → `{ result }` or `{ error }`, as one JSON string.
pub(crate) fn reply(outcome: Result<String, String>) -> String {
    match outcome {
        Ok(result) => json!({ "result": result }).to_string(),
        Err(error) => json!({ "error": error }).to_string(),
    }
}

/// The verb's words and, with `--of`, the agent whose surface it targets.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct VerbCall {
    pub args: Vec<String>,
    pub of: Option<String>,
}

fn words(items: Vec<Value>) -> Vec<String> {
    items
        .into_iter()
        .map(|item| match item {
            Value::String(text) => text,
            other => other.to_string(),
        })
        .collect()
}

/// `argsJson` is a JSON word array, or `{ args, of }` once the plugin parsed `--of`.
pub(crate) fn parse_verb_call(args_json: &str) -> Result<VerbCall, String> {
    if args_json.trim().is_empty() {
        return Ok(VerbCall::default());
    }
    match serde_json::from_str::<Value>(args_json) {
        Ok(Value::Array(items)) => Ok(VerbCall {
            args: words(items),
            of: None,
        }),
        Ok(Value::Null) => Ok(VerbCall::default()),
        Ok(Value::Object(mut object)) => {
            let args = match object.remove("args") {
                None | Some(Value::Null) => Vec::new(),
                Some(Value::Array(items)) => words(items),
                Some(_) => return Err(ARGS_SHAPE.into()),
            };
            let of = match object.remove("of") {
                None | Some(Value::Null) => None,
                Some(Value::String(of)) if !of.trim().is_empty() => Some(of.trim().to_string()),
                Some(_) => return Err(OF_USAGE.into()),
            };
            if !object.is_empty() {
                return Err(ARGS_SHAPE.into());
            }
            Ok(VerbCall { args, of })
        }
        _ => Err(ARGS_SHAPE.into()),
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

/// The owner's open surface of that declaration, if any.
pub(crate) fn caller_surface_tab(
    mux: &MultiplexerState,
    plugin_id: &str,
    surface_id: &str,
    owner_tab_id: &str,
) -> Option<String> {
    mux.all_tabs()
        .filter(|tab| tab.parent_tab_id.as_deref() == Some(owner_tab_id))
        .find(|tab| {
            tab.surface().is_some_and(|surface| {
                surface.plugin_id == plugin_id && surface.surface_id == surface_id
            })
        })
        .map(|tab| tab.id.clone())
}

fn is_agent_tab(tab: &Tab) -> bool {
    tab.surface().is_none()
        && (tab.agent_type.is_some() || tab.role.is_some() || tab.roleless_tracked)
}

fn other_workspace(raw: &str) -> String {
    format!("{raw} is in another workspace; --of only reaches agents in your own workspace")
}

/// `--of`: a tab id (a surface's id names its owner) or an exact agent title, in the caller's workspace.
pub(crate) fn resolve_owner_tab(
    mux: &MultiplexerState,
    caller_tab: &str,
    raw: &str,
) -> Result<String, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(OF_USAGE.into());
    }
    let caller = mux.get_tab(caller_tab).ok_or("the calling tab is gone")?;
    if caller.ownership.is_some() {
        return Err("--of is not available to an agent controlled from a mesh peer".into());
    }
    let workspace = mux
        .find_workspace_for_tab(caller_tab)
        .ok_or("the calling tab has no workspace")?;
    let in_workspace =
        |tab_id: &str| mux.find_workspace_for_tab(tab_id).as_deref() == Some(workspace.as_str());
    let owner = match mux.resolve_tab_id(raw).and_then(|id| mux.get_tab(&id)) {
        Some(tab) if tab.surface().is_some() => tab
            .parent_tab_id
            .clone()
            .ok_or_else(|| format!("{raw} has no owner"))?,
        Some(tab) => tab.id.clone(),
        None => {
            let named: Vec<&Tab> = mux
                .all_tabs()
                .filter(|tab| is_agent_tab(tab) && tab.title.trim().eq_ignore_ascii_case(raw))
                .collect();
            let here: Vec<&Tab> = named
                .iter()
                .copied()
                .filter(|tab| in_workspace(&tab.id))
                .collect();
            match here.as_slice() {
                [one] => one.id.clone(),
                [] if named.is_empty() => {
                    return Err(format!(
                        "no agent named {raw}; pass its exact Tab title or tab id"
                    ))
                }
                [] => return Err(other_workspace(raw)),
                many => {
                    let mut candidates: Vec<String> = many
                        .iter()
                        .map(|tab| format!("{} ({})", tab.id, tab.title))
                        .collect();
                    candidates.sort();
                    return Err(format!(
                        "{raw} names {} agents: {}; pass a tab id",
                        many.len(),
                        candidates.join(", ")
                    ));
                }
            }
        }
    };
    if !in_workspace(&owner) {
        return Err(other_workspace(raw));
    }
    Ok(owner)
}

/// What one agent verb does, decided before any runtime is touched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CallPlan {
    /// Run on this surface; `actor` is set exactly when the caller is not its owner.
    Run {
        tab_id: String,
        actor: Option<String>,
    },
    Open,
    Close {
        tab_id: String,
    },
    Answer(String),
}

/// Take control is a viewer-side mode and gates no agent, so `--of` callers share the owner's path.
pub(crate) fn plan_agent_call(
    mux: &MultiplexerState,
    plugin_id: &str,
    surface: &SurfaceDecl,
    caller_tab: &str,
    verb: &str,
    of: Option<&str>,
) -> Result<CallPlan, String> {
    let noun = surface.title.to_lowercase();
    let owner = match of {
        Some(raw) => resolve_owner_tab(mux, caller_tab, raw)?,
        None => caller_tab.to_string(),
    };
    let open = caller_surface_tab(mux, plugin_id, &surface.id, &owner);
    if owner == caller_tab {
        return match (open, verb) {
            (_, "annotate") => Err(format!(
                "annotate targets another agent's {noun}; use --of <agent>"
            )),
            (Some(tab_id), "close") => Ok(CallPlan::Close { tab_id }),
            (Some(tab_id), _) => Ok(CallPlan::Run {
                tab_id,
                actor: None,
            }),
            (None, "open") => Ok(CallPlan::Open),
            (None, "status") => Ok(CallPlan::Answer(format!(
                "no {noun} open; run `open` first"
            ))),
            (None, "close") => Ok(CallPlan::Answer(format!("no {noun} open"))),
            (None, _) => Err(format!(
                "no {noun} open for this agent; run `codeterm plugin {plugin_id} open` first"
            )),
        };
    }
    let owner_name = mux
        .get_tab(&owner)
        .map(|tab| tab.title.trim().to_string())
        .filter(|title| !title.is_empty())
        .unwrap_or_else(|| owner.clone());
    let tab_id =
        open.ok_or_else(|| format!("{owner_name} has no {noun}; it opens its own with `open`"))?;
    if verb == "close" {
        return Err(format!(
            "close only closes your own {noun}; {owner_name}'s stays open"
        ));
    }
    let actor = mux
        .get_tab(caller_tab)
        .and_then(|tab| sanitize_actor(&tab.title))
        .unwrap_or_else(|| "agent".to_string());
    Ok(CallPlan::Run {
        tab_id,
        actor: Some(actor),
    })
}

/// An annotation always goes to the surface owner, whoever wrote it.
pub(crate) fn annotation_recipient(binding: &StreamBinding) -> Result<&str, String> {
    binding
        .owner_tab_id
        .as_deref()
        .ok_or_else(|| "the surface has no owner".to_string())
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
        let VerbCall { args, of } = parse_verb_call(args_json)?;
        let declared = self
            .ext::<codeterm_plugin::surfaces::SurfaceRegistryHandle>()
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
        let plan = {
            let mux = self.mux.read();
            plan_agent_call(&mux, plugin_id, surface, &caller_tab, verb, of.as_deref())
        }?;
        let noun = surface.title.to_lowercase();
        let (tab_id, actor) = match plan {
            CallPlan::Answer(text) => return Ok(text),
            CallPlan::Close { tab_id } => {
                self.close_surface(Some(caller_tab.as_str()), &tab_id)
                    .map_err(|error| error.message())?;
                return Ok(format!("closed {noun} {tab_id}"));
            }
            CallPlan::Open => {
                let opened = self
                    .open_surface(SurfaceOpenRequest {
                        target: format!("{plugin_id}:{}", surface.id),
                        owner_tab_id: Some(caller_tab.clone()),
                        caller_tab_id: Some(caller_tab.clone()),
                        ..Default::default()
                    })
                    .map_err(|error| error.message())?;
                (opened.tab_id, None)
            }
            CallPlan::Run { tab_id, actor } => (tab_id, actor),
        };
        let (runtime, binding) = self
            .surface_runtime(&tab_id)
            .map_err(|error| error.message())?;
        let mut ctx = self.surface_call_ctx(&binding);
        ctx.actor = actor.clone();
        self.note_surface_activity(&binding.tab_id, verb, actor.as_deref());
        if verb == "annotate" {
            let annotating = runtime.clone();
            let outcome = block_on_runtime(async move { annotating.annotate(&ctx, &args).await })?;
            return Ok(self.deliver_agent_annotation(runtime.as_ref(), &binding, outcome));
        }
        let verb = verb.to_string();
        block_on_runtime(async move { runtime.agent_verb(&ctx, &verb, &args).await })
    }

    fn deliver_agent_annotation(
        &self,
        runtime: &dyn SurfaceRuntime,
        binding: &StreamBinding,
        outcome: SurfaceCallOutcome,
    ) -> String {
        let owner = binding.owner_name.as_str();
        let mut lines = Vec::new();
        for effect in outcome.effects {
            let SurfaceEffect::DeliverAnnotation { id, markdown } = effect;
            let delivered = annotation_recipient(binding)
                .and_then(|recipient| self.deliver_surface_annotation(recipient, &markdown));
            match delivered {
                Ok(()) => {
                    runtime.annotation_delivered(&id);
                    lines.push(format!("annotation sent to {owner}'s chat"));
                }
                Err(error) => {
                    log::warn!("[surface] agent annotation {id} kept for inbox: {error}");
                    lines.push(format!(
                        "{owner} could not be reached ({error}); the annotation waits in its `inbox`"
                    ));
                }
            }
        }
        if lines.is_empty() {
            "nothing to annotate".to_string()
        } else {
            lines.join("\n")
        }
    }

    fn surface_agent_guide(&self, plugin_id: &str) -> String {
        let catalog = self
            .ext::<crate::agent_commands::AgentCommandRegistryHandle>()
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
        _ => crate::async_runtime::block_on(future),
    }
}

#[cfg(test)]
mod tests;
