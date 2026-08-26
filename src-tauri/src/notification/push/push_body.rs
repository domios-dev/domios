//! The body a human wrote has to arrive as the body a human wrote.
//!
//! Two independent faults produced one mangled escalation: a body budget sized
//! for a different era, and a whitespace pass that treated a paragraph break as
//! ordinary spacing. Both are asserted here, because fixing only the length
//! would ship a long run-on line and call it done.

use super::*;
use crate::notification::channels::{discord, telegram, PushChannel};

const TAB_ID: &str = "tab_7f21c0de";
const WORKSPACE_ID: &str = "ws_lanes";
const TAB_TITLE: &str = "Worker lane";

/// Fields the push has never read and may not start reading now that there is
/// room for them. Named so a future field lands on a failing assertion.
const WITHHELD: [&str; 6] = [
    "D:/Developer/codeterm",
    "the prompt the agent was launched with",
    "the task text the plan assigned",
    "[LOG] a line from the daemon log",
    TAB_ID,
    WORKSPACE_ID,
];

fn provenance() -> NotificationProvenance {
    NotificationProvenance {
        tab_id: Some(TAB_ID.into()),
        tab_title: Some(TAB_TITLE.into()),
        workspace_id: Some(WORKSPACE_ID.into()),
        peer_name: None,
    }
}

/// The escalation that was mangled: about 2.4 KB of prose in paragraphs, ending
/// in a two-line list. Every paragraph ends in a trailing space, so tidying
/// still has something ordinary to tidy.
fn escalation_body() -> String {
    let mut body = (1..=6)
        .map(|n| {
            format!(
                "Paragraph {n}: {}",
                "the escalation says something worth reading ".repeat(9)
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    body.push_str("\n\n- the first option\n- the second option");
    body
}

/// The defect, both halves at once. A body that fitted the old cap would still
/// have arrived as one run-on line, so asserting only the length would let this
/// pass for half the wrong reason.
#[test]
fn a_real_escalation_keeps_both_its_paragraphs_and_its_length() {
    let body = escalation_body();
    let bounds = telegram::CHANNEL.bounds();
    assert!(
        body.chars().count() >= 2400,
        "the fixture is the size of the escalation that was mangled"
    );
    assert!(
        body.chars().count() > 320,
        "and it is many times past the body budget that cut it"
    );

    let rendered = render_push(
        NotifyLevel::ActionNeeded,
        "a decision is needed",
        Some(&body),
        &provenance(),
        bounds,
    );

    assert_eq!(
        rendered.matches("\n\n").count(),
        body.matches("\n\n").count(),
        "every paragraph break the agent wrote survives assembly"
    );
    assert!(
        rendered.contains("- the first option\n- the second option"),
        "and so does a single line break inside one paragraph"
    );
    for paragraph in body.split("\n\n") {
        assert!(
            rendered.contains(paragraph.trim_end()),
            "a whole paragraph went missing: {paragraph}"
        );
    }

    assert!(
        !rendered.contains(OVERFLOW_NOTE),
        "a body inside the carrying channel's budget is not truncated at all"
    );
    assert!(
        rendered.chars().count() > 2400,
        "the whole body is carried, not the first eighth of it"
    );
    assert!(rendered.chars().count() <= bounds.total);
}

/// The number belongs to the transport that has to carry the message. Two
/// channels with different ceilings cut in different places, and neither reads
/// the other's.
#[test]
fn the_bound_is_the_carrying_channels_own_not_one_global_constant() {
    let wide_bounds = telegram::CHANNEL.bounds();
    let narrow_bounds = discord::CHANNEL.bounds();
    assert_eq!(
        wide_bounds.total, 4096,
        "Telegram's own message ceiling, not a number core picked"
    );
    assert_ne!(
        narrow_bounds.total, wide_bounds.total,
        "a second channel states a ceiling of its own"
    );
    assert!(narrow_bounds.body < wide_bounds.body);

    let body = "q".repeat(narrow_bounds.body + 400);
    assert!(
        body.chars().count() < wide_bounds.body,
        "the fixture sits between the two ceilings, so only one of them cuts it"
    );

    let wide = render_push(
        NotifyLevel::Failure,
        "release",
        Some(&body),
        &provenance(),
        wide_bounds,
    );
    let narrow = render_push(
        NotifyLevel::Failure,
        "release",
        Some(&body),
        &provenance(),
        narrow_bounds,
    );

    assert_eq!(
        wide.matches('q').count(),
        body.chars().count(),
        "the wider channel carries the whole body"
    );
    assert!(!wide.contains(OVERFLOW_NOTE));
    assert_eq!(
        narrow.matches('q').count(),
        narrow_bounds.body - OVERFLOW_NOTE.chars().count(),
        "the narrower channel cuts at its own bound, never at the other one's"
    );
    assert!(narrow.contains(OVERFLOW_NOTE));
    assert!(narrow.chars().count() <= narrow_bounds.total);
}

/// A body past even the widest ceiling is cut, and the cut says where the rest
/// can be read. A silent ellipsis is output that looks complete and is not.
#[test]
fn an_over_limit_body_is_truncated_and_says_where_the_rest_is() {
    let bounds = telegram::CHANNEL.bounds();
    let body = format!("OPENING-WORDS {}", "q".repeat(bounds.body + 5000));

    let rendered = render_push(
        NotifyLevel::Failure,
        "too much to carry",
        Some(&body),
        &provenance(),
        bounds,
    );

    assert!(
        rendered.chars().count() <= bounds.total,
        "the cut lands inside the ceiling rather than past it"
    );
    assert!(
        rendered.contains("OPENING-WORDS"),
        "what is kept is the start of what was written"
    );
    assert!(
        rendered.contains(OVERFLOW_NOTE),
        "a cut body says it was cut instead of trailing off"
    );
    assert!(
        OVERFLOW_NOTE.contains("bell"),
        "and it names the surface that holds the remainder"
    );
    assert!(
        !OVERFLOW_NOTE.contains('/') && !OVERFLOW_NOTE.contains('\\'),
        "it points at the bell, not at a path nobody can open on a phone"
    );
}

/// Rejection teaches an agent to retry in smaller pieces, and retry is the spam
/// vector. Nothing on this path may refuse, at any size.
#[test]
fn an_oversized_body_still_produces_a_message_and_never_an_error() {
    let bounds = discord::CHANNEL.bounds();
    for excess in [1, bounds.total * 4] {
        let body = "q".repeat(bounds.body + excess);
        let rendered: String = render_push(
            NotifyLevel::ActionNeeded,
            "still worth sending",
            Some(&body),
            &provenance(),
            bounds,
        );
        assert!(
            !rendered.is_empty(),
            "{excess} characters over the bound still produces a message"
        );
        assert!(
            rendered.contains("still worth sending"),
            "and it is the message that was asked for"
        );
        assert!(rendered.chars().count() <= bounds.total);
    }
}

/// A wider budget is more room for the three things the push has always read,
/// never permission to read a fourth.
#[test]
fn a_wider_budget_does_not_widen_what_the_push_reads() {
    let bounds = telegram::CHANNEL.bounds();
    let title = "approval needed";
    let body = "line one\n\nline two";

    let rendered = render_push(
        NotifyLevel::ActionNeeded,
        title,
        Some(body),
        &provenance(),
        bounds,
    );

    // The equality is the pin: anything a future field added would appear here.
    assert_eq!(
        rendered,
        format!(
            "[{}] {title}\n{body}\nvia {TAB_TITLE}",
            NotifyLevel::ActionNeeded.label()
        ),
        "with room to spare the push is exactly title, body and the provenance label"
    );
    for withheld in WITHHELD {
        assert!(
            !rendered.contains(withheld),
            "{withheld} is not part of the read surface"
        );
    }
}
