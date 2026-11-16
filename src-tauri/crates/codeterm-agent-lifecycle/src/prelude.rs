//! Every lifecycle `ServiceContext` extension trait; callers import this glob or a single trait.

pub use crate::service::agent_restart::exit_recovery::general_fresh::GeneralFreshRecoveryExt;
pub use crate::service::agent_restart::exit_recovery::ExitRecoveryExt;
pub use crate::service::agent_restart::failed_launch::FailedLaunchExt;
pub use crate::service::agent_restart::fresh::FreshRestartExt;
pub use crate::service::agent_restart::model_switch::first_turn::FirstTurnSwitchExt;
pub use crate::service::agent_restart::model_switch::live_command::during_turn::DuringTurnSwitchExt;
pub use crate::service::agent_restart::model_switch::live_command::pending::PendingCommandExt;
pub use crate::service::agent_restart::model_switch::live_command::picker::CommandPickerExt;
pub use crate::service::agent_restart::model_switch::live_command::prompt::CommandPromptExt;
pub use crate::service::agent_restart::model_switch::live_command::session_identity::CommandSessionIdentityExt;
pub use crate::service::agent_restart::model_switch::live_command::LiveCommandSwitchExt;
pub use crate::service::agent_restart::model_switch::next_turn::NextTurnSwitchExt;
pub use crate::service::agent_restart::model_switch::staging::StagedSwitchExt;
pub use crate::service::agent_restart::model_switch::ModelSwitchExt;
pub use crate::service::agent_restart::options::RestartOptionsExt;
pub use crate::service::agent_restart::provider_switch::general::GeneralProviderSwitchExt;
pub use crate::service::agent_restart::provider_switch::input::ProviderSwitchInputExt;
pub use crate::service::agent_restart::provider_switch::operations::ProviderSwitchOperationsExt;
pub use crate::service::agent_restart::provider_switch::ProviderSwitchExt;
pub use crate::service::agent_restart::runtime::RuntimeRestartExt;
pub use crate::service::agent_restart::shutdown::RestartShutdownExt;
pub use crate::service::agent_restart::switch_confirmation::SwitchConfirmationExt;
pub use crate::service::agent_restart::unverified_shell::UnverifiedShellExt;
#[cfg(any(test, feature = "test-support"))]
pub use crate::service::agent_restart::unverified_shell::UnverifiedShellFixtureExt;
pub use crate::service::agent_restart::AgentRestartExt;
pub use crate::service::agent_restart::ProviderSwitchClaimExt;
pub use crate::service::restore_exited_member::RestoreExitedMemberExt;
pub use crate::service::worker_interruption::WorkerInterruptionExt;
#[cfg(any(test, feature = "test-support"))]
pub use crate::service::worker_interruption::WorkerInterruptionFixtureExt;
