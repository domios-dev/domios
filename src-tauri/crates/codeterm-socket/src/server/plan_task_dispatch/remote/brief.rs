use codeterm_plan::plan::PlanSkillManifestEntry;
use codeterm_plan::service_plan::{plan_skills, PlanStore};
use sha2::{Digest, Sha256};

use super::super::invalid_dispatch;
use crate::server::JsonRpcError;

const MAX_SKILL_CONTEXT_BYTES: usize = 256 * 1024;

pub(super) fn plan_skill_context(
    store: &PlanStore,
    plan_id: &str,
    manifest: &[PlanSkillManifestEntry],
) -> Result<String, JsonRpcError> {
    render_skill_context(plan_id, manifest, |slug| {
        plan_skills::show_plan_skill(store, plan_id, slug)
            .map(|skill| skill.body)
            .map_err(crate::server::plan::plan_rpc_err)
    })
}

fn render_skill_context(
    plan_id: &str,
    manifest: &[PlanSkillManifestEntry],
    mut read_body: impl FnMut(&str) -> Result<String, JsonRpcError>,
) -> Result<String, JsonRpcError> {
    if manifest.is_empty() {
        return Ok(String::new());
    }
    let mut context = format!(
        "## Plan skill manifest\nOwning plan: `{plan_id}` on the origin peer. After the remote ACK and before repository or tool actions, choose applicability explicitly from the purpose metadata below. Byte-completely read each applicable skill's supplied body in this dispatch snapshot. These bodies replace local plan skill list/show commands; this peer has no local plan.\n\n"
    );
    for skill in manifest {
        let body = read_body(&skill.slug)?;
        if body.len() > MAX_SKILL_CONTEXT_BYTES.saturating_sub(context.len()) {
            return Err(invalid_dispatch(
                "remote plan skill context exceeds 256 KiB; reduce the kept skill roster before dispatch",
            ));
        }
        if format!("sha256:{:x}", Sha256::digest(body.as_bytes())) != skill.content_digest {
            return Err(invalid_dispatch(
                "plan skill changed while preparing remote dispatch; prepare a fresh dispatch snapshot",
            ));
        }
        let fence = "`".repeat(
            body.split(|ch| ch != '`')
                .map(str::len)
                .max()
                .unwrap_or(0)
                .max(2)
                + 1,
        );
        context.push_str(&format!(
            "### `{}` | {} | {} | `{}`\nSkill body ({} UTF-8 bytes):\n{fence}markdown\n",
            skill.slug,
            skill.title,
            skill.purpose,
            skill.content_digest,
            body.len(),
        ));
        context.push_str(&body);
        context.push_str(&format!("\n{fence}\n\n"));
        if context.len() > MAX_SKILL_CONTEXT_BYTES {
            return Err(invalid_dispatch(
                "remote plan skill context exceeds 256 KiB; reduce the kept skill roster before dispatch",
            ));
        }
    }
    Ok(context)
}

pub(in crate::server::plan_task_dispatch) fn remote_worker_brief(
    brief: String,
    plan_id: &str,
    task_id: &str,
    attempt_id: &str,
    origin_id: &str,
    origin_tab_id: &str,
    skill_context: &str,
) -> String {
    let remote_ack = format!(
        "Highest-priority startup action: run `domios plan task remote-ack {plan_id} {task_id} {attempt_id} --mesh {origin_id}` before any work. Do not continue until it prints the task id. This peer has no local plan, so local agent ACK/report and plan commands do not apply here."
    );
    let ack = codeterm_plan::service_plan::worker_identity::WORKER_ACK_INSTRUCTION;
    let remote_report = format!(
        "domios plan task remote-report {plan_id} {task_id} {attempt_id} --message-file <report-path> --sha <lane-head> --mesh {origin_id}"
    );
    let local_report =
        codeterm_plan::service_plan::task_attempts::task_report_command(plan_id, task_id);
    let brief = brief.replace(&local_report, &remote_report);
    let brief = if brief.contains(ack) {
        brief.replacen(ack, &remote_ack, 1)
    } else {
        format!("{remote_ack}\n\n{brief}")
    };
    format!(
        "{brief}\n\n{skill_context}\n## Remote Worker operations\n\
         This assignment and its strides are the task snapshot; use them instead of local task show or plan reads. For additional plan context or skill assets, ask the origin Orchestrator.\n\
         Maintain this tab's checklist locally with `domios todo list` and `domios todo write --from-pad todos.json`; use plain items without origin-plan task links. Share the checklist and progress by writing a message in your pad and running `domios send --tab {origin_tab_id} --mesh {origin_id} --from-pad <message.md>`. This replaces todo submit and plan record on this peer; the origin Orchestrator records plan strides.\n\
         Work only in the assigned peer lane. Lane Workers do not run plan task extend-files.\n\
         These remote operations take precedence over local plan instructions in task history, skills or role guidance. If unfinished or blocked, send the origin Orchestrator the actual evidence and remaining work through the same message route; it decides task state.\n\n\
         Remote final report: write the report in your pad, then run `{remote_report}` with its absolute report path and your literal lane commit SHA. It persists your report on the origin attempt and moves the task to in_review.\n"
    )
}

#[cfg(test)]
mod tests;
