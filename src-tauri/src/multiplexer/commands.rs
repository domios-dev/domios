use std::sync::Arc;

/// Shared multiplexer state, thread-safe.
pub type MuxState = Arc<crate::lock_hold::TimedRwLock<crate::multiplexer::state::MultiplexerState>>;
