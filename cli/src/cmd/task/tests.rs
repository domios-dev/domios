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
