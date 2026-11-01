mod composition;
pub mod mem_backend;
pub mod startup;
pub use composition::ComposedService;
pub mod bootstrap;
mod bundle;
pub mod headless_update;
pub mod launch;
mod launch_ownership;
pub mod native_host;
mod native_inventory;
pub mod native_update;
mod runtime;
pub use codeterm_paths::instance_identity::ownership_registry;
pub use runtime::run;
mod resources;
mod worker_observer;

#[cfg(test)]
#[path = "../build_frontend.rs"]
mod build_frontend;

mod exact_quit;
mod native_application_host;
pub mod pty_child;
