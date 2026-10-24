#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeSource {
    AgentNotifySend,
    UserConsent,
    System,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeDestination {
    Bell,
    Log,
}

pub fn destination(source: NoticeSource, live_agent_caller: bool) -> NoticeDestination {
    match (source, live_agent_caller) {
        (NoticeSource::UserConsent, _) | (NoticeSource::AgentNotifySend, true) => {
            NoticeDestination::Bell
        }
        _ => NoticeDestination::Log,
    }
}

#[cfg(test)]
mod tests;
