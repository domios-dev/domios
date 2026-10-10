use crate::args::plan::PlanReviewAction;
use crate::client::RpcClient;
use serde_json::{json, Value};

use super::{call_plan_rpc, inject_trusted_session, maybe_inject_caller_tab, owner_from_env};

pub(super) fn handle(
    client: &mut dyn RpcClient,
    json_output: bool,
    action: PlanReviewAction,
) -> Result<Option<Value>, String> {
    match action {
        PlanReviewAction::Status { plan_id } => {
            let mut params = json!({});
            if let Some(plan_id) = plan_id {
                params["plan_id"] = json!(plan_id);
            }
            let val = call_plan_rpc(client, "plan.review.status", params)?;
            if json_output {
                return Ok(Some(val));
            }
            print!("{}", render_status(&val));
            Ok(None)
        }
        PlanReviewAction::Waves { plan_id } => {
            let mut params = json!({});
            if let Some(plan_id) = plan_id {
                params["plan_id"] = json!(plan_id);
            }
            let val = call_plan_rpc(client, "plan.review.waves", params)?;
            if json_output {
                return Ok(Some(val));
            }
            print!("{}", render_waves(&val));
            Ok(None)
        }
        PlanReviewAction::Override {
            plan_id,
            task_id,
            owner,
            reason,
            reproduction,
        } => {
            let mut params = json!({
                "task_id": task_id,
                "owner": owner,
                "reason": reason,
                "reproduction": reproduction,
                "from": owner_from_env(),
            });
            if let Some(plan_id) = plan_id {
                params["plan_id"] = json!(plan_id);
            }
            maybe_inject_caller_tab(&mut params);
            call_plan_rpc(client, "plan.review.override", params).map(Some)
        }
        PlanReviewAction::ReconcileCredit { plan_id, reason } => {
            let mut params = json!({
                "reason": reason,
            });
            if let Some(plan_id) = plan_id {
                params["plan_id"] = json!(plan_id);
            }
            inject_trusted_session(&mut params);
            call_plan_rpc(client, "plan.review.reconcile-credit", params).map(Some)
        }
        PlanReviewAction::AuthorizeCapacity {
            plan_id,
            task_id,
            class,
        } => {
            let mut params = json!({
                "task_id": task_id,
                "class": class,
            });
            if let Some(plan_id) = plan_id {
                params["plan_id"] = json!(plan_id);
            }
            inject_trusted_session(&mut params);
            call_plan_rpc(client, "plan.review.authorize-capacity", params).map(Some)
        }
        PlanReviewAction::AuthorizeReporter {
            plan_id,
            reporter_tab_id,
            class,
        } => {
            let mut params = json!({
                "plan_id": plan_id,
                "reporter_tab_id": reporter_tab_id,
                "class": class,
            });
            inject_trusted_session(&mut params);
            call_plan_rpc(client, "plan.review.authorize-reporter", params).map(Some)
        }
        PlanReviewAction::Accept {
            plan_id,
            task_id,
            note,
            note_from_pad,
            changed_executable_surfaces,
            distinct_oracle,
            residual_risk,
        } => {
            let mut params = json!({
                "task_id": task_id,
                "note": resolve_note(note, note_from_pad)?,
                "changed_executable_surfaces": changed_executable_surfaces,
                "distinct_oracle": distinct_oracle,
                "from": owner_from_env(),
            });
            if let Some(plan_id) = plan_id {
                params["plan_id"] = json!(plan_id);
            }
            if let Some(residual_risk) = residual_risk {
                params["residual_risk"] = json!(residual_risk);
            }
            maybe_inject_caller_tab(&mut params);
            call_plan_rpc(client, "plan.review.accept", params).map(Some)
        }
        PlanReviewAction::Bounce {
            plan_id,
            task_id,
            note,
            note_from_pad,
            changed_executable_surfaces,
            distinct_oracle,
        } => {
            let mut params = json!({
                "task_id": task_id,
                "note": resolve_note(note, note_from_pad)?,
                "changed_executable_surfaces": changed_executable_surfaces,
                "distinct_oracle": distinct_oracle,
                "from": owner_from_env(),
            });
            if let Some(plan_id) = plan_id {
                params["plan_id"] = json!(plan_id);
            }
            inject_trusted_session(&mut params);
            call_plan_rpc(client, "plan.review.bounce", params).map(Some)
        }
        PlanReviewAction::Hold {
            plan_id,
            task_id,
            note,
            note_from_pad,
        } => {
            let mut params = json!({
                "task_id": task_id,
                "note": resolve_note(note, note_from_pad)?,
                "from": owner_from_env(),
            });
            if let Some(plan_id) = plan_id {
                params["plan_id"] = json!(plan_id);
            }
            maybe_inject_caller_tab(&mut params);
            call_plan_rpc(client, "plan.review.hold", params).map(Some)
        }
    }
}

fn resolve_note(note: Option<String>, from_pad: Option<String>) -> Result<String, String> {
    resolve_note_with(note, from_pad, crate::cmd::payload_source::read_from_pad)
}

fn resolve_note_with(
    note: Option<String>,
    from_pad: Option<String>,
    read_pad: impl FnOnce(&str) -> Result<String, String>,
) -> Result<String, String> {
    match (note, from_pad) {
        (None, Some(path)) => read_pad(&path),
        (Some(note), None) => {
            if let Some(error) = crate::cmd::quoted_handoff::inline_refusal(
                crate::cmd::quoted_handoff::INLINE_SOURCE,
                &note,
                "--note-from-pad <relative path>",
            ) {
                return Err(error);
            }
            Ok(note)
        }
        _ => Err("provide exactly one of --note or --note-from-pad".into()),
    }
}

fn render_status(value: &Value) -> String {
    let Some(gate) = value.get("review_gate") else {
        return "No review gate status available.\n".into();
    };
    let wip_count = gate.get("wip_count").and_then(|v| v.as_u64()).unwrap_or(0);
    let wip_limit = gate.get("wip_limit").and_then(|v| v.as_u64()).unwrap_or(0);
    let credit = gate.get("credit").and_then(|v| v.as_i64()).unwrap_or(0);
    let blocked = gate
        .get("blocked")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let mut out =
        format!("Review WIP: {wip_count}/{wip_limit}  credit: {credit}  blocked: {blocked}\n");
    if let Some(reason) = gate.get("reason").and_then(|v| v.as_str()) {
        out.push_str(&format!("Reason: {reason}\n"));
    }
    out
}

fn render_waves(value: &Value) -> String {
    let waves = value
        .get("waves")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if waves.is_empty() {
        return "No QA waves pending.\n".into();
    }
    let mut out = String::new();
    for wave in &waves {
        let key = wave
            .get("group_key")
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        let label = wave.get("label").and_then(|v| v.as_str()).unwrap_or(key);
        let count = wave
            .get("task_ids")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        out.push_str(&format!("- {key} ({label}): {count} task(s)\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn review_cmd_verdicts_require_one_note_source() {
        for verdict in ["bounce", "hold", "accept"] {
            for args in [
                vec![],
                vec!["--note", "scalar", "--note-from-pad", "note.md"],
            ] {
                let mut argv = vec!["codeterm", "plan", "review", verdict, "pl_1", "task_1"];
                argv.extend(args);
                assert!(crate::args::cli::Cli::try_parse_from(argv).is_err());
            }
            for source in ["--note", "--note-from-pad"] {
                assert!(crate::args::cli::Cli::try_parse_from([
                    "codeterm", "plan", "review", verdict, "pl_1", "task_1", source, "note.md",
                ])
                .is_ok());
            }
        }
    }

    #[test]
    fn review_cmd_pad_note_preserves_bytes_and_containment() {
        let pad = tempfile::tempdir().unwrap();
        let body = "Evidence: `code` $value \"quoted\"\nSecond line\n".repeat(40);
        std::fs::write(pad.path().join("note.md"), &body).unwrap();
        let read = |path: &str| crate::cmd::payload_source::read_from_pad_root(pad.path(), path);
        assert_eq!(
            resolve_note_with(None, Some("note.md".into()), read).unwrap(),
            body
        );
        assert!(resolve_note_with(None, Some("../note.md".into()), read).is_err());
        assert!(resolve_note_with(None, Some("missing.md".into()), read).is_err());
    }

    #[test]
    fn review_cmd_refuses_inline_before_verdict_transport() {
        struct NoCalls;
        impl RpcClient for NoCalls {
            fn call_detailed(
                &mut self,
                _: &str,
                _: Option<Value>,
            ) -> Result<Value, crate::client::SocketCallError> {
                panic!("refused note must not reach transport");
            }
        }
        for verdict in ["bounce", "hold", "accept"] {
            let cli = crate::args::cli::Cli::try_parse_from([
                "codeterm",
                "plan",
                "review",
                verdict,
                "pl_1",
                "task_1",
                "--note",
                "first\nsecond",
            ])
            .unwrap();
            let crate::args::cli::Commands::Plan {
                action: crate::args::plan::PlanAction::Review { action },
            } = cli.command
            else {
                panic!("expected review action");
            };
            assert!(handle(&mut NoCalls, true, action)
                .unwrap_err()
                .contains(crate::cmd::quoted_handoff::REFUSAL_CODE));
        }
    }

    #[test]
    fn review_cmd_inline_note_refuses_authored_payload() {
        for note in ["short\nmultiline".into(), "long".repeat(100)] {
            assert!(crate::cmd::quoted_handoff::is_authored_payload(&note));
            let error = resolve_note(Some(note), None).unwrap_err();
            assert!(error.contains(crate::cmd::quoted_handoff::REFUSAL_CODE));
        }
        assert_eq!(resolve_note(Some("scalar".into()), None).unwrap(), "scalar");
    }
}
