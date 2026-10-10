use super::ReportStatus;

const VERDICT_COMMAND: &str = "Next: codeterm plan review accept|bounce";
const IN_REVIEW_SINCE: &str = "in review since";
const REPORTED_AT: &str = "reported at";

pub(super) fn append(
    body: &str,
    plan_id: Option<&str>,
    task_id: Option<&str>,
    status: Option<ReportStatus>,
    timestamp: u64,
) -> String {
    let (Some(plan), Some(task)) = (plan_id, task_id) else {
        return body.to_string();
    };
    let command = format!("{VERDICT_COMMAND} {plan} {task} --note \"...\"");
    let since = if status == Some(ReportStatus::InReview) {
        IN_REVIEW_SINCE
    } else {
        REPORTED_AT
    };
    let hour = (timestamp / 3_600) % 24;
    let minute = (timestamp / 60) % 60;
    let footer = format!("{command} ({since} {hour:02}:{minute:02} UTC)");
    let mut content = body.trim_end();
    while let Some((previous, last)) = content.rsplit_once('\n') {
        if !last.starts_with(&command) {
            break;
        }
        content = previous.trim_end();
    }
    if content.starts_with(&command) && !content.contains('\n') {
        content = "";
    }
    if content.is_empty() {
        footer
    } else {
        format!("{content}\n{footer}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::reports::{ReportDetails, ReportStore};

    #[test]
    fn report_verdict_footer_renders_once_at_the_end() {
        let mut report = ReportStore::new(1).add_detailed(
            "parent",
            "worker",
            "Worker",
            "opaque evidence\nNext: unrelated instruction",
            ReportDetails {
                plan_id: Some("pl_render".into()),
                task_id: Some("task_render".into()),
                status: Some(ReportStatus::InReview),
                ..Default::default()
            },
            None,
        );
        report.timestamp = 49_020;
        let body = report.inject_body();
        let (evidence, footer) = body.rsplit_once('\n').unwrap();
        assert!(evidence.ends_with(&report.message));
        assert!(footer.starts_with(&format!(
            "{VERDICT_COMMAND} pl_render task_render --note \"...\""
        )));
        assert!(footer.ends_with("13:37 UTC)"));
        assert_eq!(body.matches(VERDICT_COMMAND).count(), 1);
        assert_eq!(report.inject_body(), body);

        report.message.push('\n');
        report.message.push_str(footer);
        report.message.push('\n');
        report.message.push_str(footer);
        assert_eq!(report.inject_body(), body);
    }

    #[test]
    fn unattached_report_has_no_verdict_footer() {
        for (plan, task) in [
            (None, None),
            (Some("pl_render"), None),
            (None, Some("task_render")),
        ] {
            assert_eq!(
                append("opaque evidence\n", plan, task, None, 0),
                "opaque evidence\n"
            );
        }
    }

    #[test]
    fn footer_only_body_is_idempotent() {
        let footer = append("", Some("pl_render"), Some("task_render"), None, 0);
        assert_eq!(
            append(&footer, Some("pl_render"), Some("task_render"), None, 0),
            footer
        );
    }

    #[test]
    fn only_review_reports_label_the_timestamp_as_review_time() {
        for status in [
            None,
            Some(ReportStatus::Partial),
            Some(ReportStatus::Blocked),
            Some(ReportStatus::InReview),
        ] {
            let body = append(
                "evidence",
                Some("pl_render"),
                Some("task_render"),
                status,
                0,
            );
            let label = if status == Some(ReportStatus::InReview) {
                IN_REVIEW_SINCE
            } else {
                REPORTED_AT
            };
            assert!(body.ends_with(&format!("({label} 00:00 UTC)")));
        }
    }
}
