use super::utility_shell_purpose;
use codeterm_service_core::multiplexer::state::MultiplexerState;
use codeterm_service_core::multiplexer::types::{ShellPurpose, Tab};
use codeterm_service_core::service::messenger::{MessageEnvelope, Messenger};

fn shell(purpose: ShellPurpose) -> Tab {
    let (mut mux, first) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&first).unwrap();
    let id = mux.create_tab(&workspace).unwrap();
    let mut tab = mux.get_tab(&id).unwrap().clone();
    tab.purpose = purpose;
    assert!(tab.is_utility_shell());
    tab
}

fn opening_tag(tab: &Tab) -> String {
    Messenger::from(
        MessageEnvelope::tab("Builder", "abcd1234-ffff")
            .purpose(utility_shell_purpose(tab))
            .from_agent(Some(tab.agent_type.is_some())),
    )
    .build()
}

#[test]
fn build_shell_sender_carries_purpose() {
    let tab = shell(ShellPurpose::Build);
    assert_eq!(utility_shell_purpose(&tab).as_deref(), Some("build"));
    assert_eq!(
        opening_tag(&tab),
        r#"<domios from="tab" label="Builder" tab="abcd1234" purpose="build">"#
    );
}

#[test]
fn plain_shell_and_agent_senders_omit_purpose() {
    let plain = shell(ShellPurpose::Shell);
    assert_eq!(utility_shell_purpose(&plain), None);
    assert!(!opening_tag(&plain).contains("purpose="));

    let mut agent = shell(ShellPurpose::Build);
    agent.agent_type = Some("claude".into());
    assert_eq!(utility_shell_purpose(&agent), None);
    assert!(!opening_tag(&agent).contains("purpose="));

    let mut booting = shell(ShellPurpose::Qa);
    booting.agent_booting = true;
    assert_eq!(utility_shell_purpose(&booting), None);
}
