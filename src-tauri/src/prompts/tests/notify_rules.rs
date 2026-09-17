//! What every surface that teaches notify must carry, and what the surfaces
//! that never notify must not. Delivery by constant identity: the wording lives
//! in the .md files, and only the fragments' arrival is asserted here.

use super::*;

/// The seeded `codeterm` skill body is compiled into codeterm-mem, which
/// `test-one app` cannot reach. Read the one file both crates load, so this
/// stays a single-crate gate on the same bytes mem seeds from.
const SEED_SKILL_CODETERM: &str = include_str!("../../../prompts/seed/skill_codeterm.md");

/// The long flag the ask clause teaches, named once so the clause and the
/// manifest are compared against the same token rather than two spellings.
const ASK_FLAG: &str = "--choice";

/// Every surface that teaches an agent when to escalate to the user, as the
/// agent receives it rather than as the repository files it is built from.
///
/// `compose_system_prompt` joins the primer and the rule into one prompt, so
/// requiring the clause in each file would require it twice in what arrives —
/// which is the duplication these clauses were consolidated to remove. Counting
/// the composed text catches the missing case and the doubled case alike, which
/// per-file counting cannot. The seed skill is a separate distribution and
/// carries its own copy.
fn notify_escalation_surfaces() -> [(&'static str, String); 2] {
    [
        (
            "composed system prompt",
            format!("{AGENT_CODETERM_PRIMER}\n{CODETERM_RULE}"),
        ),
        ("seed/skill_codeterm.md", SEED_SKILL_CODETERM.to_string()),
    ]
}

/// Delivery, by constant identity. The clause is an opaque token here: what
/// it says belongs to the .md files, and that each surface receives it once
/// is the only mechanism this test owns.
#[test]
fn notify_ask_guidance_reaches_every_escalation_surface() {
    let clause = NOTIFY_ASK_CLAUSE;
    assert!(
        !clause.trim().is_empty(),
        "an empty fragment proves nothing"
    );

    for (name, surface) in notify_escalation_surfaces() {
        assert_eq!(
            surface.matches(clause).count(),
            1,
            "{name} must carry the ask clause exactly once"
        );
    }
}

/// The clause names a flag; the committed manifest is the authority on
/// whether that flag exists. Guidance cannot document a flag the CLI lacks.
#[test]
fn notify_ask_guidance_names_a_flag_the_manifest_has() {
    let root: serde_json::Value =
        serde_json::from_str(CLI_MANIFEST).expect("cli-manifest.json must parse");
    let send = subcommand(&root, "notify")
        .and_then(|notify| subcommand(notify, "send"))
        .expect("the manifest must carry notify send");

    assert!(NOTIFY_ASK_CLAUSE.contains(ASK_FLAG));
    assert!(
        flag_names(send).contains(&ASK_FLAG.trim_start_matches('-')),
        "notify send must accept the flag the ask clause teaches"
    );
}

/// The addition is a clause on a restriction, never a replacement for it:
/// the surface that carries the clause must still state the rule that keeps
/// the escalation rare. Both are named by constant, not by their sentences.
#[test]
fn notify_ask_guidance_leaves_the_rare_escalation_rule_standing() {
    assert!(!NOTIFY_RARE_ESCALATION_RULE.trim().is_empty());
    assert_eq!(
        AGENT_CODETERM_PRIMER
            .matches(NOTIFY_RARE_ESCALATION_RULE)
            .count(),
        1
    );
    assert!(AGENT_CODETERM_PRIMER.contains(NOTIFY_ASK_CLAUSE));
}

/// Worker and Planner tabs never notify the user directly, so their role
/// surfaces must not gain the clause. A copy there would read as an
/// invitation to the one role the restriction excludes.
#[test]
fn notify_ask_guidance_stays_out_of_surfaces_that_never_notify() {
    for (name, surface) in surfaces_that_never_notify() {
        assert!(
            !surface.contains(NOTIFY_ASK_CLAUSE),
            "{name} must not restate the ask clause"
        );
    }
}

/// The roles the notify restriction excludes. Named once so the negative
/// control and the ask-clause control cannot drift apart.
fn surfaces_that_never_notify() -> [(&'static str, &'static str); 3] {
    [
        ("worker_suffix.md", WORKER_SUFFIX),
        ("worker_codeterm_context.md", WORKER_CODETERM_CONTEXT),
        ("planner_suffix.md", PLANNER_SUFFIX),
    ]
}

/// Delivery, by constant identity. What the relaxation says belongs to the .md
/// files; that every surface teaching notify receives it once is the mechanism
/// this test owns.
#[test]
fn verbose_completion_relaxation_reaches_every_notify_surface() {
    let clause = NOTIFY_VERBOSE_COMPLETION_CLAUSE;
    assert!(
        !clause.trim().is_empty(),
        "an empty fragment proves nothing"
    );

    for (name, surface) in notify_escalation_surfaces() {
        assert_eq!(
            surface.matches(clause).count(),
            1,
            "{name} must carry the verbose completion clause exactly once"
        );
    }
}

/// The same surfaces must explain the `unseen` report, so an agent meeting it
/// under the id reads information rather than an error it should retry.
#[test]
fn verbose_completion_unseen_clause_reaches_every_notify_surface() {
    let clause = NOTIFY_UNSEEN_CLAUSE;
    assert!(
        !clause.trim().is_empty(),
        "an empty fragment proves nothing"
    );

    for (name, surface) in notify_escalation_surfaces() {
        assert_eq!(
            surface.matches(clause).count(),
            1,
            "{name} must carry the unseen clause exactly once"
        );
    }
}

/// A relaxation, not an invitation. The permission cannot be delivered without
/// the restriction it carries, and the two rules that did not relax must still
/// stand where they stood. All three are named by constant, never by sentence.
#[test]
fn verbose_completion_leaves_the_surviving_restrictions_standing() {
    assert!(!NOTIFY_NO_RUNNING_COMMENTARY_RULE.trim().is_empty());
    assert!(
        NOTIFY_VERBOSE_COMPLETION_CLAUSE.contains(NOTIFY_NO_RUNNING_COMMENTARY_RULE),
        "the permission must carry its own restriction"
    );

    for (name, surface) in notify_escalation_surfaces() {
        assert_eq!(
            surface.matches(NOTIFY_NO_RUNNING_COMMENTARY_RULE).count(),
            1,
            "{name} must still forbid a running commentary"
        );
    }

    assert!(!NOTIFY_WORKER_PLANNER_SILENCE_RULE.trim().is_empty());
    assert_eq!(
        AGENT_CODETERM_PRIMER
            .matches(NOTIFY_WORKER_PLANNER_SILENCE_RULE)
            .count(),
        1,
        "the primer must still send Workers and Planners through their channel"
    );
    assert_eq!(
        AGENT_CODETERM_PRIMER
            .matches(NOTIFY_RARE_ESCALATION_RULE)
            .count(),
        1,
        "the two-case rule for the escalating levels must still stand"
    );
}

/// The relaxation is scoped to the surfaces that teach notify. A copy on a
/// Worker or Planner surface would read as permission to the roles excluded.
#[test]
fn verbose_completion_stays_out_of_surfaces_that_never_notify() {
    for (name, surface) in surfaces_that_never_notify() {
        assert!(
            !surface.contains(NOTIFY_VERBOSE_COMPLETION_CLAUSE),
            "{name} must not restate the verbose completion clause"
        );
        assert!(
            !surface.contains(NOTIFY_UNSEEN_CLAUSE),
            "{name} must not restate the unseen clause"
        );
    }
}
