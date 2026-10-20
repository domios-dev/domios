use super::*;

#[test]
fn lane_clean_clap_defaults_to_preview_and_requires_explicit_confirmation() {
    for (flags, expected) in [
        (vec!["--clean"], (true, false, false)),
        (vec!["--clean", "--confirm"], (true, true, false)),
        (vec!["--clean", "--fetch"], (true, false, true)),
    ] {
        let mut args = vec!["codeterm", "plan", "lanes", "plan"];
        args.extend(flags);
        let parsed = Cli::try_parse_from(args).unwrap();
        let Commands::Plan {
            action:
                PlanAction::Lanes {
                    plan_id,
                    clean,
                    confirm,
                    fetch,
                    stale,
                },
        } = parsed.command
        else {
            panic!()
        };
        assert_eq!(plan_id.as_deref(), Some("plan"));
        assert_eq!((clean, confirm, fetch), expected);
        assert!(!stale);
    }
    for flags in [
        vec!["--confirm"],
        vec!["--fetch"],
        vec!["--clean", "--stale"],
    ] {
        let mut args = vec!["codeterm", "plan", "lanes", "plan"];
        args.extend(flags);
        assert!(Cli::try_parse_from(args).is_err());
    }
}
