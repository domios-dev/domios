//! The core's service module, with the lifted lifecycle modules beside it.

pub(crate) use codeterm_service_core::service::*;

pub mod agent_restart;
pub mod restore_exited_member;
#[cfg(test)]
pub(crate) mod test_fixture;
pub mod worker_interruption;
