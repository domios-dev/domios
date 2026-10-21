mod composition;
pub mod startup;
pub use composition::{compose, ComposedService};
pub mod bootstrap;
mod bundle;
pub mod launch;
pub mod native_host;
mod native_inventory;
pub mod native_update;
mod runtime;
pub use runtime::{ownership_registry, run};
mod resources;
pub(crate) mod upgrade_from_1_14_9;
mod worker_observer;

mod exact_quit;

extern crate domios_api as codeterm_api; // rename-bridge
extern crate domios_billing as codeterm_billing; // rename-bridge
extern crate domios_build_info as codeterm_build_info; // rename-bridge
extern crate domios_commands as codeterm_commands; // rename-bridge
extern crate domios_events as codeterm_events; // rename-bridge
extern crate domios_frontend_bundle as codeterm_frontend_bundle; // rename-bridge
extern crate domios_frontend_shell as codeterm_frontend_shell; // rename-bridge
extern crate domios_orchestration as codeterm_orchestration; // rename-bridge
extern crate domios_plan as codeterm_plan; // rename-bridge
extern crate domios_plugin as codeterm_plugin; // rename-bridge
extern crate domios_plugins as codeterm_plugins; // rename-bridge
extern crate domios_process_identity as codeterm_process_identity; // rename-bridge
extern crate domios_sentinel_host as codeterm_sentinel_host; // rename-bridge
extern crate domios_service_core as codeterm_service_core; // rename-bridge
extern crate domios_socket as codeterm_socket; // rename-bridge
extern crate domios_types as codeterm_types; // rename-bridge
extern crate domios_update as codeterm_update; // rename-bridge
extern crate domios_util as codeterm_util; // rename-bridge
