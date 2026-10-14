use super::model::{matches_source, matches_user_turn, PendingReplyRelay, RelaySource};
use crate::agent::provider::ReplyRelayEvidence;
use crate::agent::reports::ReportStatus;

pub(super) fn prepare_completed_reply(
    pending: &PendingReplyRelay,
    source: RelaySource<'_>,
    evidence: Option<&ReplyRelayEvidence>,
) -> (Option<ReplyRelayEvidence>, Option<ReportStatus>) {
    let mut evidence = evidence.filter(|evidence| evidence.is_valid()).cloned();
    let Some(ReplyRelayEvidence::Answered(turn)) = evidence.as_mut() else {
        return (evidence, None);
    };
    if !turn.complete
        || !matches_source(pending, source)
        || !matches_user_turn(
            pending,
            &turn.user_turn_id,
            turn.user_record_start,
            &turn.user_text,
        )
    {
        return (evidence, None);
    }
    let Some((status, body)) = parse_report_marker(&turn.answer) else {
        return (evidence, None);
    };
    turn.answer = body.to_string();
    (evidence, Some(status))
}

fn parse_report_marker(answer: &str) -> Option<(ReportStatus, &str)> {
    let (line, body) = answer.split_once('\n')?;
    let line = line.strip_suffix('\r').unwrap_or(line);
    if !line.starts_with("<domios ") || !line.ends_with('>') {
        return None;
    }
    let tagged = format!("{}{}</domios>", "<domios from=\"report\"", &line[7..]);
    let parsed = codeterm_wire::domios::parse_message(&tagged)?;
    if parsed.attr("type") != Some("report") || !parsed.content.is_empty() {
        return None;
    }
    let status = match parsed.attr("status") {
        Some("blocked") => ReportStatus::Blocked,
        Some("partial") => ReportStatus::Partial,
        _ => ReportStatus::InReview,
    };
    Some((status, body))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn marker_status_and_stripped_body_follow_strict_first_line_grammar() {
        for (status, expected) in [
            ("in_review", ReportStatus::InReview),
            ("blocked", ReportStatus::Blocked),
            ("partial", ReportStatus::Partial),
            ("unknown", ReportStatus::InReview),
        ] {
            let answer = format!("<domios type=\"report\" status=\"{status}\">\nopaque body");
            assert_eq!(
                parse_report_marker(&answer),
                Some((expected, "opaque body"))
            );
        }
        assert_eq!(
            parse_report_marker("<domios type=\"report\">\r\nbody"),
            Some((ReportStatus::InReview, "body"))
        );
        for answer in [
            "ordinary",
            "prefix\n<domios type=\"report\">\nbody",
            "<domios type='report'>\nbody",
            "<domios type=\"report\" status=blocked>\nbody",
            "<domios type=\"report\" type=\"report\">\nbody",
            "<domios type=\"report\">suffix\nbody",
        ] {
            assert_eq!(parse_report_marker(answer), None);
        }
    }
}
