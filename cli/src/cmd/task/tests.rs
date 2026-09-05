mod external_ref_render_tests {
    use crate::cmd::task::render_external_refs;
    use serde_json::json;

    #[test]
    fn task_show_renders_canonical_external_ref_and_relationship() {
        let lines = render_external_refs(&json!({
            "external_refs": [{
                "canonical_url": "https://github.com/rollacode/codeterm-releases/issues/17",
                "relationship": "fixes"
            }]
        }));
        assert_eq!(
            lines,
            ["external_ref: https://github.com/rollacode/codeterm-releases/issues/17 (fixes)"]
        );
    }
}

mod siblings_render_tests {
    use crate::cmd::task::{
        is_closed_sibling_status, short_display_id, truncate_label, TaskShowOpts,
    };
    use serde_json::json;

    fn render_siblings_line(items: &[serde_json::Value], opts: &TaskShowOpts) -> String {
        if !opts.list_siblings {
            let open = items
                .iter()
                .filter(|item| {
                    !item
                        .get("status")
                        .and_then(|v| v.as_str())
                        .is_some_and(is_closed_sibling_status)
                })
                .count();
            let closed = items.len().saturating_sub(open);
            return if closed > 0 {
                format!("siblings: {open} open (+ {closed} done/cancelled); use --siblings to list")
            } else {
                format!("siblings: {open} open; use --siblings to list")
            };
        }
        let rendered = items
            .iter()
            .filter(|item| {
                opts.include_done_siblings
                    || !item
                        .get("status")
                        .and_then(|v| v.as_str())
                        .is_some_and(is_closed_sibling_status)
            })
            .map(|item| {
                let id = item.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                let status = item.get("status").and_then(|v| v.as_str()).unwrap_or("?");
                format!(
                    "{} {} ({status})",
                    short_display_id(id),
                    truncate_label(name, 48)
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        if rendered.is_empty() {
            "siblings: none".into()
        } else {
            format!("siblings: {rendered}")
        }
    }

    #[test]
    fn default_siblings_is_one_summary_line() {
        let siblings: Vec<_> = (0..160)
            .map(|i| {
                json!({
                    "id": format!("task_{i:03}"),
                    "name": format!("Sibling {i}"),
                    "status": if i % 3 == 0 { "done" } else { "pending" }
                })
            })
            .collect();
        let line = render_siblings_line(
            &siblings,
            &TaskShowOpts {
                list_siblings: false,
                include_done_siblings: false,
            },
        );
        assert!(!line.contains("Sibling 0"));
        assert!(line.contains("use --siblings to list"));
    }

    #[test]
    fn siblings_flag_uses_short_ids_and_truncated_names() {
        let line = render_siblings_line(
            &[json!({
                "id": "task_abcdef012345",
                "name": "A very long sibling name that should be truncated for terminal display",
                "status": "pending"
            })],
            &TaskShowOpts {
                list_siblings: true,
                include_done_siblings: false,
            },
        );
        assert!(line.contains("task_abcdef"));
        assert!(line.contains('…'));
    }
}

#[test]
fn diagnostics_that_speak_task_show_routing_values_are_rendered_or_unset() {
    use crate::cmd::task::task_routing_lines;
    use serde_json::json;

    let values = task_routing_lines(&json!({
        "role_guidance": "Keep it bounded",
        "personality": "Brynhild",
        "model": "codex:gpt-5"
    }));
    assert!(values.iter().any(|line| line.contains("Keep it bounded")));
    assert!(values.iter().any(|line| line.contains("Brynhild")));
    assert!(values.iter().any(|line| line.contains("codex:gpt-5")));
    let unset = task_routing_lines(&json!({}));
    assert_eq!(
        unset.iter().filter(|line| line.contains("(unset)")).count(),
        3
    );
}

mod task_quality_render_tests {
    use crate::cmd::task::{executor_role_line, task_quality_lines};
    use serde_json::json;

    #[test]
    fn surfaces_supplied_criteria_test_plan_qa_plan_and_correctness() {
        let task = json!({
            "acceptance_criteria": ["c1-value", "c2-value"],
            "test_plan": "cargo test tp-value",
            "qa_plan": "cargo test qa-value",
            "correctness": "corr-value",
            "executor_role": "role-value",
        });
        let lines = task_quality_lines(&task);
        assert!(lines.iter().any(|l| l == "criteria: c1-value; c2-value"));
        assert!(lines.iter().any(|l| l == "test_plan: cargo test tp-value"));
        assert!(lines.iter().any(|l| l == "qa_plan: cargo test qa-value"));
        assert!(lines.iter().any(|l| l == "correctness: corr-value"));
        assert_eq!(
            executor_role_line(&task).as_deref(),
            Some("executor_role: role-value")
        );
    }

    #[test]
    fn omits_blank_or_absent_quality_fields() {
        let lines = task_quality_lines(&json!({ "test_plan": "" }));
        assert!(lines.is_empty(), "{lines:?}");
    }
}

mod executor_role_render_tests {
    use crate::cmd::task::executor_role_line;
    use serde_json::json;

    #[test]
    fn renders_line_when_set() {
        let task = json!({ "id": "task_1", "executor_role": "qa" });
        assert_eq!(
            executor_role_line(&task).as_deref(),
            Some("executor_role: qa")
        );
    }

    #[test]
    fn omits_line_when_absent_or_blank() {
        assert!(executor_role_line(&json!({ "id": "task_1" })).is_none());
        assert!(executor_role_line(&json!({ "executor_role": "  " })).is_none());
        assert!(executor_role_line(&json!({ "executor_role": null })).is_none());
    }
}

mod task_presentation_render_tests {
    use crate::cmd::task::task_presentation_line;
    use serde_json::json;

    #[test]
    fn renders_waiting_dependency_as_pending_not_paused() {
        let task = json!({
            "presentation": "waiting_on_dependencies",
            "dependency_waits": [{"task_id": "task_before", "name": "Before"}],
        });
        assert_eq!(
            task_presentation_line(&task).as_deref(),
            Some("presentation: waiting on dependency — task_before (Before)")
        );
    }
}

mod strides_render_tests {
    use crate::cmd::task::render_strides;
    use serde_json::json;

    #[test]
    fn omits_when_no_milestones() {
        assert!(render_strides(&json!({ "id": "task_1" })).is_none());
        assert!(render_strides(&json!({ "id": "task_1", "milestones": [] })).is_none());
    }

    #[test]
    fn renders_newest_first_with_author_kind_ts_text() {
        let task = json!({
            "milestones": [
                { "id": "ms_1", "kind": "status", "ts": 1_000u64,
                  "from": { "title": "Elgar" }, "text": "started" },
                { "id": "ms_2", "kind": "report", "ts": 2_000u64,
                  "from": { "title": "Elgar" }, "text": "done" },
            ]
        });
        let block = render_strides(&task).expect("block");
        let lines: Vec<&str> = block.lines().collect();
        assert_eq!(lines[0], "strides:");
        assert!(lines[1].contains("[report]"), "newest first: {}", lines[1]);
        assert!(lines[1].contains("Elgar"));
        assert!(lines[1].ends_with(": done"));
        assert!(lines[2].contains("[status]"));
    }

    #[test]
    fn survives_legacy_and_empty_shapes() {
        let task = json!({
            "milestones": [
                {},
                { "kind": "note", "ts": 0u64 },
                { "kind": "status", "ts": 5_000u64, "from": { "tab_id": "tab_abcdef01234" } },
            ]
        });
        let block = render_strides(&task).expect("block");
        assert!(block.starts_with("strides:"));
        assert_eq!(block.lines().count(), 4);
        assert!(block.contains("[?]"));
        assert!(block.contains("tab_abcdef"));
    }
}

mod error_contract_tests {
    use crate::args::inbox::InboxAction;
    use crate::args::task::{CanvasAction, TaskAction};
    use crate::client::{RpcClient, RpcError, SocketCallError};
    use crate::cmd::print::{write_cli_error, CliError, CLI_ERROR_EXIT_CODE};
    use crate::cmd::task::{run_canvas, run_inbox, run_task};
    use serde_json::Value;

    /// Every JSON-RPC code the daemon uses starts with this; asserting its
    /// absence is structural, unlike pinning a message phrase.
    const RPC_CODE_MARKER: &str = "-32";
    const DAEMON_MESSAGE: &str = "task 'zzzzzzzz' not found";

    #[derive(Default)]
    struct FailingRpc {
        calls: Vec<String>,
        remote: bool,
    }

    impl FailingRpc {
        fn local() -> Self {
            Self::default()
        }

        fn remote() -> Self {
            Self {
                remote: true,
                ..Self::default()
            }
        }
    }

    impl RpcClient for FailingRpc {
        fn call_detailed(
            &mut self,
            method: &str,
            _params: Option<Value>,
        ) -> Result<Value, SocketCallError> {
            self.calls.push(method.to_string());
            Err(SocketCallError::Rpc(RpcError {
                code: -32602,
                message: DAEMON_MESSAGE.into(),
                data: None,
            }))
        }

        fn is_remote(&self) -> bool {
            self.remote
        }
    }

    fn rendered(json: bool, message: &str) -> (String, String) {
        let mut out = Vec::new();
        let mut err = Vec::new();
        write_cli_error(json, &CliError::new(message), &mut out, &mut err);
        (
            String::from_utf8(out).expect("stdout is utf8"),
            String::from_utf8(err).expect("stderr is utf8"),
        )
    }

    fn show_error(client: &mut FailingRpc) -> String {
        run_task(
            client,
            false,
            TaskAction::Show {
                task_id: "task_zzzzzzzz".into(),
                siblings: false,
                all: false,
            },
        )
        .err()
        .expect("task show surfaces the daemon rejection")
    }

    #[test]
    fn human_failure_writes_nothing_to_stdout() {
        let (out, err) = rendered(false, DAEMON_MESSAGE);

        assert!(out.is_empty(), "stdout must stay parseable: {out}");
        assert_eq!(err.lines().count(), 1, "one failure is one line: {err}");
        assert!(err.ends_with(&format!("{DAEMON_MESSAGE}\n")), "{err}");
    }

    #[test]
    fn json_failure_puts_a_structural_envelope_on_stdout() {
        let (out, err) = rendered(true, DAEMON_MESSAGE);

        assert!(err.is_empty(), "the envelope is the whole report: {err}");
        let envelope: serde_json::Value =
            serde_json::from_str(&out).expect("stdout carries exactly one JSON envelope");
        assert_eq!(envelope["ok"], serde_json::Value::Bool(false));
        assert_eq!(envelope["kind"], "error");
        assert_eq!(envelope["error"], DAEMON_MESSAGE);
    }

    /// Exit-code parity itself is owned by `print.rs`, whose `cli_error_exit_code`
    /// is private; what this lane can still pin is that neither mode goes silent.
    #[test]
    fn both_modes_report_on_exactly_one_stream_and_exit_non_zero() {
        let (json_out, json_err) = rendered(true, DAEMON_MESSAGE);
        let (human_out, human_err) = rendered(false, DAEMON_MESSAGE);

        assert!(!json_out.is_empty() && json_err.is_empty(), "{json_err}");
        assert!(human_out.is_empty() && !human_err.is_empty(), "{human_out}");
        assert_ne!(CLI_ERROR_EXIT_CODE, 0);
    }

    #[test]
    fn task_show_rejection_reaches_the_contract_without_the_raw_rpc_code() {
        let mut client = FailingRpc::local();
        let message = show_error(&mut client);

        assert_eq!(client.calls, ["task.show"]);
        assert!(message.contains(DAEMON_MESSAGE), "{message}");
        assert!(!message.contains(RPC_CODE_MARKER), "{message}");

        let (out, err) = rendered(false, &message);
        assert!(out.is_empty(), "{out}");
        assert!(!err.contains(RPC_CODE_MARKER), "{err}");
    }

    #[test]
    fn task_list_rejection_reaches_the_contract() {
        let mut client = FailingRpc::local();
        let message = run_task(
            &mut client,
            false,
            TaskAction::List {
                active: false,
                all: true,
                mine: false,
                unassigned: false,
                workspace: false,
                plan: None,
                status: None,
            },
        )
        .err()
        .expect("task list surfaces the daemon rejection");

        assert_eq!(client.calls, ["task.list"]);
        let (out, _) = rendered(false, &message);
        assert!(out.is_empty(), "{out}");
    }

    #[test]
    fn locally_rejected_task_take_never_reaches_the_daemon_and_still_exits_one() {
        let mut client = FailingRpc::remote();
        let message = run_task(
            &mut client,
            false,
            TaskAction::Take {
                task_id: "task_1".into(),
            },
        )
        .err()
        .expect("task take over mesh is rejected before any RPC");

        assert!(client.calls.is_empty(), "{:?}", client.calls);
        let (out, err) = rendered(true, &message);
        assert!(err.is_empty(), "{err}");
        let envelope: serde_json::Value = serde_json::from_str(&out).expect("one JSON envelope");
        assert_eq!(envelope["ok"], serde_json::Value::Bool(false));
        assert_eq!(envelope["code"], serde_json::Value::Null);
    }

    #[test]
    fn inbox_add_rejection_reaches_the_contract() {
        let mut client = FailingRpc::local();
        let message = run_inbox(&mut client, inbox_add(None))
            .expect_err("inbox add surfaces the daemon rejection");

        assert_eq!(client.calls, ["task.add"]);
        let (out, err) = rendered(false, &message);
        assert!(out.is_empty(), "{out}");
        assert_eq!(err.lines().count(), 1, "{err}");
        assert!(err.ends_with(&format!("{DAEMON_MESSAGE}\n")), "{err}");
    }

    #[test]
    fn inbox_add_unreadable_body_file_is_rejected_before_any_rpc() {
        let mut client = FailingRpc::local();
        let missing = std::env::temp_dir()
            .join("codeterm-absent-inbox-dir")
            .join("body.md");
        assert!(!missing.exists(), "the fixture must name an absent path");
        let message = run_inbox(&mut client, inbox_add(Some(missing.display().to_string())))
            .expect_err("an unreadable --body-file is rejected locally");

        assert!(client.calls.is_empty(), "{:?}", client.calls);
        let (out, err) = rendered(false, &message);
        assert!(out.is_empty(), "{out}");
        assert_eq!(err.lines().count(), 1, "{err}");
    }

    #[test]
    fn canvas_focus_rejection_reaches_the_contract() {
        let mut client = FailingRpc::local();
        let message = run_canvas(
            &mut client,
            CanvasAction::Focus {
                task: Some("task_1".into()),
                tab: None,
                plan: None,
            },
        )
        .expect_err("canvas focus surfaces the daemon rejection");

        assert_eq!(client.calls, ["canvas.focus"]);
        let (out, err) = rendered(true, &message);
        assert!(err.is_empty(), "{err}");
        let envelope: serde_json::Value = serde_json::from_str(&out).expect("one JSON envelope");
        assert_eq!(envelope["ok"], serde_json::Value::Bool(false));
    }

    fn inbox_add(body_file: Option<String>) -> InboxAction {
        InboxAction::Add {
            name: "probe".into(),
            kind: None,
            model: None,
            why: None,
            why_file: None,
            criterion: Vec::new(),
            test_plan: None,
            qa_plan: None,
            qa_plan_file: None,
            correctness: None,
            body: None,
            body_file,
            intent: None,
            intent_file: None,
            execution_intent: None,
            assign_tab: None,
        }
    }
}

mod executor_render_tests {
    use crate::cmd::task::executor_line;
    use serde_json::json;

    /// A mesh-staffed task's executor is a field, not a sentence in a pause
    /// reason. Asserted on the rendered fields, not on the surrounding phrasing.
    #[test]
    fn task_show_renders_a_mesh_executor_tab_and_peer() {
        let line = executor_line(&json!({
            "assigned_tab_id": "ed4cbff7",
            "peer": "Mac-Air",
            "taken_by_role": "infra-dev",
            "lane": { "tab_id": "ed4cbff7" },
        }))
        .expect("an assigned task must render its executor");
        assert!(line.contains("ed4cbff7"), "{line}");
        assert!(line.contains("Mac-Air"), "{line}");
        assert!(line.contains("infra-dev"), "{line}");
    }

    /// The lane binding alone is enough: an executor recorded there is still an
    /// executor, and must not go unrendered.
    #[test]
    fn task_show_falls_back_to_the_lane_tab_binding() {
        let line = executor_line(&json!({
            "assigned_tab_id": null,
            "lane": { "tab_id": "7f2a91b0" },
        }))
        .expect("a lane-bound executor must still render");
        assert!(line.contains("7f2a91b0"), "{line}");
    }

    /// No executor recorded anywhere renders nothing, rather than a line
    /// claiming an empty tab.
    #[test]
    fn task_show_renders_no_executor_when_none_is_recorded() {
        assert!(executor_line(&json!({
            "assigned_tab_id": null,
            "peer": null,
            "lane": { "tab_id": null },
        }))
        .is_none());
        assert!(executor_line(&json!({
            "assigned_tab_id": "   ",
            "lane": { "tab_id": "" },
        }))
        .is_none());
    }
}
