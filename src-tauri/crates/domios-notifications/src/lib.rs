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

extern crate domios_agent_model as codeterm_agent_model; // rename-bridge
extern crate domios_bridge as codeterm_bridge; // rename-bridge
extern crate domios_config as codeterm_config; // rename-bridge
extern crate domios_notification_core as codeterm_notification_core; // rename-bridge
extern crate domios_paths as codeterm_paths; // rename-bridge
extern crate domios_plugin as codeterm_plugin; // rename-bridge
extern crate domios_pty as codeterm_pty; // rename-bridge
extern crate domios_types as codeterm_types; // rename-bridge
#[cfg(test)]
extern crate domios_util as codeterm_util; // rename-bridge
