use std::sync::Arc;

/// Shared multiplexer state, thread-safe.
pub type MuxState =
    Arc<codeterm_util::lock_hold::TimedRwLock<crate::multiplexer::state::MultiplexerState>>;
