#[path = "crash_exit.rs"]
pub mod exit;

pub mod shutdown {
    pub use super::exit::is_client_exit_pending as is_shutting_down;
}
