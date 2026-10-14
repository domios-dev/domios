use super::UserSendAuthority;

#[derive(Clone, Debug, PartialEq, Eq)]
struct ReadyUserLaunch {
    pty_id: String,
    binding_nonce: String,
    pty_session_generation: u64,
    provider_id: String,
}

#[derive(Default)]
pub(super) struct ReadyLaunchLatch {
    ready: Option<ReadyUserLaunch>,
}

impl ReadyLaunchLatch {
    pub(super) fn mark_ready(&mut self, authority: &UserSendAuthority) -> bool {
        let Some(key) = ReadyUserLaunch::from_authority(authority) else {
            return false;
        };
        self.ready = Some(key);
        true
    }

    pub(super) fn is_ready(&self, authority: &UserSendAuthority) -> bool {
        ReadyUserLaunch::from_authority(authority)
            .is_some_and(|key| self.ready.as_ref() == Some(&key))
    }
}

impl ReadyUserLaunch {
    fn from_authority(authority: &UserSendAuthority) -> Option<Self> {
        Some(Self {
            pty_id: authority.pty_id.clone()?,
            binding_nonce: authority.binding_nonce.clone(),
            pty_session_generation: authority.pty_session_generation?,
            provider_id: authority.provider_id.clone(),
        })
    }
}

#[cfg(test)]
mod tests;
