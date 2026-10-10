use super::*;

#[test]
fn tab_release_ownership_defaults_to_the_calling_tab_and_accepts_an_explicit_id() {
    let current = Cli::try_parse_from(["codeterm", "tab", "release-ownership"])
        .expect("release-ownership defaults to the calling tab");
    assert!(matches!(
        current.command,
        Commands::Tab {
            action: TabAction::ReleaseOwnership { tab: None }
        }
    ));

    let explicit = Cli::try_parse_from(["codeterm", "tab", "release-ownership", "--tab", "tab-a"])
        .expect("release-ownership accepts an explicit tab");
    assert!(matches!(
        explicit.command,
        Commands::Tab {
            action: TabAction::ReleaseOwnership { ref tab }
        } if tab.as_deref() == Some("tab-a")
    ));
}
