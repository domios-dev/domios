use super::*;

#[test]
fn tab_adopt_agent_requires_provider_and_parses_force() {
    assert!(Cli::try_parse_from(["codeterm", "tab", "adopt-agent"]).is_err());

    let parsed = Cli::try_parse_from([
        "codeterm",
        "tab",
        "adopt-agent",
        "--provider",
        "mock-pty",
        "--force",
        "--tab",
        "tab-a",
    ])
    .expect("adopt-agent parses");
    assert!(matches!(
        parsed.command,
        Commands::Tab {
            action: TabAction::AdoptAgent {
                ref provider,
                force: true,
                tab: Some(ref tab),
            },
        } if provider == "mock-pty" && tab == "tab-a"
    ));
}
