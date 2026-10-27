use super::*;
use crate::args::radar::RadarAction;

#[derive(Parser)]
struct RadarOnly {
    #[command(subcommand)]
    action: RadarAction,
}

#[test]
fn radar_add_parses_repeated_tags_and_source_identity() {
    let parsed = RadarOnly::try_parse_from([
        "radar",
        "add",
        "--title",
        "Noticed",
        "--tag",
        "ux",
        "--tag",
        "mesh",
        "--source-provider",
        "github",
        "--source-id",
        "acme/widgets#4",
    ])
    .expect("radar add parses");
    match parsed.action {
        RadarAction::Add {
            title,
            tag,
            source_provider,
            source_id,
            ..
        } => {
            assert_eq!(title, "Noticed");
            assert_eq!(tag, ["ux", "mesh"]);
            assert_eq!(source_provider.as_deref(), Some("github"));
            assert_eq!(source_id.as_deref(), Some("acme/widgets#4"));
        }
        _ => panic!("expected radar add"),
    }
}

#[test]
fn radar_refuses_conflicting_workspace_and_tag_flags() {
    for argv in [
        vec!["radar", "list", "--project", "/w", "--all-workspaces"],
        vec!["radar", "edit", "rd_1", "--tag", "ux", "--clear-tags"],
        vec!["radar", "list", "--status", "done"],
        vec!["radar", "pull", "rd_1"],
    ] {
        assert!(
            RadarOnly::try_parse_from(argv.clone()).is_err(),
            "{argv:?} must not parse"
        );
    }
}
