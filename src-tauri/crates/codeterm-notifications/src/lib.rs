pub mod integration_silence;
pub mod notification;
pub mod notification_create;
pub mod notification_push;
pub mod provider_hooks;

pub(crate) mod service {
    pub(crate) use crate::integration_silence;
    #[cfg(test)]
    pub(crate) use crate::notification_create as notification;
    #[cfg(test)]
    pub(crate) use crate::notification_push;
}
pub(crate) mod agent {
    pub(crate) use codeterm_agent_model::agent::{provider, registry};
}
pub(crate) mod session {
    pub(crate) use codeterm_pty::session::lifecycle;
}
pub(crate) use codeterm_paths as paths;

pub(crate) use codeterm_config as config;

pub mod bridge_threads;
