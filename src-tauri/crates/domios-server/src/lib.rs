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
