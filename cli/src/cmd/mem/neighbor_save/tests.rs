use clap::Parser;

#[test]
fn neighbor_save_help_documents_body_area_and_repeated_tags() {
    let help = match crate::Cli::try_parse_from(["domios", "mem", "neighbor-save", "--help"]) {
        Err(error) => error,
        Ok(_) => panic!("help parsed as a command"),
    };
    assert_eq!(help.kind(), clap::error::ErrorKind::DisplayHelp);
    for flag in [
        "--kind",
        "--area",
        "--title",
        "--from-pad",
        "--summary",
        "--tag",
    ] {
        assert!(help.to_string().contains(flag));
    }
    assert!(crate::Cli::try_parse_from([
        "domios",
        "mem",
        "neighbor-save",
        "--kind",
        "rule",
        "--area",
        "project",
        "--title",
        "opaque",
        "--from-pad",
        "-"
    ])
    .is_err());
    let parsed = crate::Cli::try_parse_from([
        "domios",
        "mem",
        "neighbor-save",
        "--kind",
        "note",
        "--area",
        "project",
        "--title",
        "opaque",
        "--from-pad",
        "body.md",
        "--tag",
        "one",
        "--tag",
        "two",
    ])
    .unwrap();
    let crate::Commands::Mem {
        action: crate::MemAction::NeighborSave { tags, from_pad, .. },
    } = parsed.command
    else {
        panic!("wrong command")
    };
    assert_eq!(tags, vec!["one", "two"]);
    assert_eq!(from_pad, "body.md");
}

#[test]
fn neighbor_payload_carries_content_without_authority_or_mutation_options() {
    assert_eq!(
        super::payload(
            "fact",
            "project",
            "opaque",
            "body".into(),
            &None,
            &["tag".into()]
        ),
        serde_json::json!({"kind":"fact","area":"project","title":"opaque","body":"body","summary":null,"tags":["tag"]})
    );
}
