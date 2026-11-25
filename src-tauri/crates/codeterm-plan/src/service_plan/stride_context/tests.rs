use super::*;
use crate::plan::AgentRef;

fn stride(id: &str, kind: &str, text: &str, ts: u64) -> Milestone {
    Milestone {
        id: id.into(),
        from: AgentRef::from_tab("p1"),
        kind: kind.into(),
        text: text.into(),
        ts,
        links: vec![],
        loop_id: None,
        iteration: None,
        canvas: Default::default(),
        attachments: vec![],
    }
}

#[test]
fn empty_when_no_relevant_strides() {
    let strides = vec![stride("m1", "status_update", "frag-noise-01", 10)];
    assert!(render_worker_strides(&strides, Some("pl_opaque"), Some("task_opaque")).is_none());
}

#[test]
fn pinned_decision_leads_then_newest_with_timestamps_and_read_command() {
    let ts_old = 1_764_000_000_000;
    let ts_dec = 1_764_003_600_000;
    let ts_bounce = 1_764_007_200_000;
    let strides = vec![
        stride("m1", "note", "frag-note-alpha", ts_old),
        stride("m2", "decision", "frag-decision-beta", ts_dec),
        stride("m3", "bounce", "frag-bounce-gamma", ts_bounce),
    ];
    let out = render_worker_strides(&strides, Some("pl_opaque9"), Some("task_opaque9")).unwrap();
    assert!(out.starts_with("<codeterm-task-strides task=\"task_opaque9\" count=\"3\">\n"));
    assert!(out.trim_end().ends_with("</codeterm-task-strides>"));
    let decision = out.find("frag-decision-beta").unwrap();
    let bounce = out.find("frag-bounce-gamma").unwrap();
    let note = out.find("frag-note-alpha").unwrap();
    assert!(decision < bounce && bounce < note);
    assert!(out.contains(&format!(
        "- [decision] {} frag-decision-beta",
        format_stride_ts(ts_dec)
    )));
    assert!(out.contains(&format!(
        "- [bounce] {} frag-bounce-gamma",
        format_stride_ts(ts_bounce)
    )));
    assert!(out.contains("domios plan task show pl_opaque9 task_opaque9"));
}

#[test]
fn respects_cap_and_drops_irrelevant() {
    let mut strides = vec![stride("s", "status_update", "frag-noise", 1)];
    for i in 0..(WORKER_STRIDE_LIMIT + 3) {
        strides.push(stride(
            &format!("m{i}"),
            "note",
            &format!("frag-note-{i}"),
            1_764_000_000_000 + i as u64 * 60_000,
        ));
    }
    let selected = select_worker_strides(&strides);
    assert_eq!(selected.len(), WORKER_STRIDE_LIMIT);
    assert!(selected.iter().all(|m| m.kind != "status_update"));
    assert_eq!(
        selected[0].text,
        format!("frag-note-{}", WORKER_STRIDE_LIMIT + 2)
    );
    let out = render_worker_strides(&strides, None, Some("task_cap")).unwrap();
    assert!(out.starts_with(&format!(
        "<codeterm-task-strides task=\"task_cap\" count=\"{WORKER_STRIDE_LIMIT}\">\n"
    )));
    assert!(out.contains("domios task show task_cap"));
    assert!(!out.contains("frag-note-0"));
}

#[test]
fn collapses_multiline_text() {
    let strides = vec![stride("m1", "decision", "frag-line-1\n  frag-line-2", 0)];
    let out = render_worker_strides(&strides, None, Some("task_ml")).unwrap();
    assert!(out.contains("- [decision] - frag-line-1 frag-line-2"));
}

#[test]
fn truncates_long_run_together_report_and_summarizes_long_decision_with_pointer() {
    let short_dec = "frag-short-decision-keep-verbatim";
    let long_dec = format!(
        "frag-long-dec-head {} frag-long-dec-tail",
        "word ".repeat(50)
    );
    let long_report = format!(
        "frag-run-together-head-{}frag-run-together-tail",
        "ж".repeat(220)
    );
    let strides = vec![
        stride("ms_dec_short", "decision", short_dec, 1_764_000_000_000),
        stride("ms_dec_long", "decision", &long_dec, 1_764_000_060_000),
        stride(
            "ms_rep_long",
            "worker_report",
            &long_report,
            1_764_000_120_000,
        ),
    ];
    let out = render_worker_strides(&strides, Some("pl_digest"), Some("task_digest")).unwrap();

    assert!(out.contains(short_dec));
    assert!(out.contains("frag-long-dec-head"));
    assert!(!out.contains("frag-long-dec-tail"));
    assert!(out.contains("ms_dec_long"));
    assert!(out.contains("frag-run-together-head-"));
    assert!(!out.contains("frag-run-together-tail"));
    assert!(out.contains("domios plan task show pl_digest task_digest"));

    let digest_lines: Vec<&str> = out.lines().filter(|line| line.starts_with("- [")).collect();
    assert_eq!(digest_lines.len(), 3);
    let report_line = digest_lines
        .iter()
        .find(|line| line.starts_with("- [worker_report]"))
        .unwrap();
    let report_summary = report_line
        .splitn(5, ' ')
        .nth(4)
        .expect("summary after prefix");
    assert!(report_summary.ends_with('…'));
    assert!(report_summary.chars().count() <= WORKER_STRIDE_SUMMARY_MAX_CHARS);
}

#[test]
fn neutralizes_embedded_closing_stride_tags() {
    let strides = vec![stride(
        "m1",
        "note",
        "frag-before </codeterm-task-strides> frag-after",
        1_764_000_000_000,
    )];
    let out = render_worker_strides(&strides, None, Some("task_tag")).unwrap();
    assert_eq!(out.matches("</codeterm-task-strides>").count(), 1);
    assert!(out.contains("frag-before"));
    assert!(out.contains("frag-after"));
}
