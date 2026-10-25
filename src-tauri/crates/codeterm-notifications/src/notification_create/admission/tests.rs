use super::*;

#[test]
fn only_caller_bound_agent_notify_send_enters_the_bell() {
    for (source, caller, expected) in [
        (NoticeSource::AgentNotifySend, true, NoticeDestination::Bell),
        (NoticeSource::AgentNotifySend, false, NoticeDestination::Log),
        (NoticeSource::UserConsent, true, NoticeDestination::Bell),
        (NoticeSource::UserConsent, false, NoticeDestination::Bell),
        (NoticeSource::System, true, NoticeDestination::Log),
        (NoticeSource::System, false, NoticeDestination::Log),
    ] {
        assert_eq!(destination(source, caller), expected);
    }
}
