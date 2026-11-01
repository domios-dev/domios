use super::{step_agent_run::StepRunRequest, ServiceContext};
use codeterm_plugin::blueprint_step::StepInputContext;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StartOptions {
    prompt: String,
    cwd: String,
    model: Option<String>,
    timeout_ms: u64,
    sandbox: Option<String>,
}

impl ServiceContext {
    pub(super) fn plugin_step_agent_start(
        &self,
        plugin: &str,
        context: &StepInputContext,
        json: &str,
    ) -> String {
        let start = || -> Result<String, String> {
            if !context.submitting
                || context.plugin != plugin
                || context.cancellation.is_cancelled()
                || std::time::Instant::now() >= context.deadline
            {
                return Err("agentRun.start requires an active step submit".into());
            }
            let opts: StartOptions =
                serde_json::from_str(json).map_err(|_| "invalid agent run options")?;
            if !(1..=3_600_000).contains(&opts.timeout_ms)
                || opts.cwd.len() > 1024
                || opts.model.as_ref().is_some_and(|model| model.len() > 128)
            {
                return Err("invalid agent run bounds".into());
            }
            if opts
                .sandbox
                .as_deref()
                .is_some_and(|sandbox| sandbox != "read-only")
            {
                return Err("unsupported step sandbox".into());
            }
            let cwd = authored_cwd(context, &opts.cwd)?;
            self.start_step_agent_run(StepRunRequest {
                plugin: plugin.into(),
                operation: context.request.context.operation.clone(),
                workspace: context.request.context.scope.workspace.clone(),
                team: context.request.context.scope.team.clone(),
                owner: context.request.context.owner.clone(),
                label: context.request.context.node_label.clone(),
                prompt: opts.prompt,
                model: opts.model,
                cwd,
                timeout_ms: opts.timeout_ms,
                sandbox: opts.sandbox,
            })
        };
        match start() {
            Ok(run) => serde_json::json!({"run":run}).to_string(),
            Err(error) => serde_json::json!({"error":error}).to_string(),
        }
    }
}

fn authored_cwd(context: &StepInputContext, requested: &str) -> Result<String, String> {
    let authored = context
        .request
        .params
        .get("cwd")
        .and_then(serde_json::Value::as_str)
        .ok_or("step cwd is missing")?;
    if requested != authored {
        return Err("agent cwd differs from the authored step cwd".into());
    }
    super::step_agent_run::working_dir(authored)
}

#[cfg(test)]
mod tests;
