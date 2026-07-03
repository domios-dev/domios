use crate::args::cli::{Cli, Commands};
use crate::args::plan::{PlanAction, PlanSkillAction};
use clap::Parser;

fn skill_action(args: &[&str]) -> PlanSkillAction {
    let mut argv = vec!["codeterm", "plan", "skill"];
    argv.extend_from_slice(args);
    match Cli::try_parse_from(argv).expect("parses").command {
        Commands::Plan {
            action: PlanAction::Skill { action },
        } => action,
        _ => panic!("expected plan skill"),
    }
}

#[test]
fn new_parses_slug_and_defaults() {
    match skill_action(&["new", "deploy-staging"]) {
        PlanSkillAction::New {
            slug,
            plan,
            rationale,
            ..
        } => {
            assert_eq!(slug, "deploy-staging");
            assert_eq!(plan, None);
            assert_eq!(rationale, None);
        }
        _ => panic!("expected new"),
    }
}

#[test]
fn list_allows_zero_flag_plan() {
    match skill_action(&["list"]) {
        PlanSkillAction::List { plan } => assert_eq!(plan, None),
        _ => panic!("expected list"),
    }
}

#[test]
fn show_takes_slug_and_optional_plan() {
    match skill_action(&["show", "deploy", "--plan", "plan_x"]) {
        PlanSkillAction::Show { slug, plan } => {
            assert_eq!(slug, "deploy");
            assert_eq!(plan.as_deref(), Some("plan_x"));
        }
        _ => panic!("expected show"),
    }
}

#[test]
fn edit_defaults_verdict_keep_and_requires_rationale() {
    match skill_action(&["edit", "deploy", "--rationale", "tightened steps"]) {
        PlanSkillAction::Edit {
            slug,
            verdict,
            rationale,
            body_file,
            ..
        } => {
            assert_eq!(slug, "deploy");
            assert_eq!(verdict, "keep");
            assert_eq!(rationale, "tightened steps");
            assert_eq!(body_file, None);
        }
        _ => panic!("expected edit"),
    }
}

#[test]
fn edit_accepts_superseded_verdict() {
    match skill_action(&[
        "edit",
        "deploy",
        "--verdict",
        "superseded",
        "--rationale",
        "replaced by v2",
    ]) {
        PlanSkillAction::Edit { verdict, .. } => assert_eq!(verdict, "superseded"),
        _ => panic!("expected edit"),
    }
}

#[test]
fn edit_without_rationale_fails() {
    let err = Cli::try_parse_from(["codeterm", "plan", "skill", "edit", "deploy"]);
    assert!(err.is_err(), "rationale is required for edit");
}
