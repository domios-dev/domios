#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeSource {
    AgentNotifySend,
    System,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeDestination {
    Bell,
    Log,
}

pub fn destination(source: NoticeSource, live_agent_caller: bool) -> NoticeDestination {
    match (source, live_agent_caller) {
        (NoticeSource::AgentNotifySend, true) => NoticeDestination::Bell,
        _ => NoticeDestination::Log,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_caller_bound_agent_notify_send_enters_the_bell() {
        for (source, caller, expected) in [
            (NoticeSource::AgentNotifySend, true, NoticeDestination::Bell),
            (NoticeSource::AgentNotifySend, false, NoticeDestination::Log),
            (NoticeSource::System, true, NoticeDestination::Log),
            (NoticeSource::System, false, NoticeDestination::Log),
        ] {
            assert_eq!(destination(source, caller), expected);
        }
    }
}
