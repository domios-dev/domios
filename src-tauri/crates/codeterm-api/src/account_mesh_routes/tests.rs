use super::*;
use codeterm_service_core::service::tab_credentials::TabCaller;

#[test]
fn agent_summary_is_read_only_and_owner_mutations_stay_private() {
    let agent = Extension(CallerIdentity::Tab(TabCaller {
        tab_id: "agent".into(),
        binding_nonce: "session".into(),
    }));
    assert!(admit_summary(Some(&agent)).is_ok());
    assert!(admit(Some(&agent)).is_err());
    assert!(admit_summary(Some(&Extension(CallerIdentity::Local))).is_ok());
    assert!(admit_summary(None).is_err());
    assert!(admit_summary(Some(&Extension(CallerIdentity::ServicePush {
        service_id: "service".into()
    })))
    .is_err());
    assert!(admit_summary(Some(&Extension(CallerIdentity::Peer {
        instance_id: "peer".into(),
        access_level: codeterm_mesh::types::AccessLevel::Full,
        relay_protocol_version: codeterm_mesh::types::RELAY_PROTOCOL_VERSION
    })))
    .is_err());
}
