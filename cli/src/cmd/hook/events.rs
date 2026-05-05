//! Per-event handlers. Phase 2 implements only `pre_tool_use`; the other
//! three events (`session_start`, `user_prompt_submit`, `pre_compact`)
//! are stubs that return empty stdout — Phase 5 fills them in.
//!
//! Each handler signature is `(input: &str) -> String`. The dispatcher
//! prints the returned string to stdout. An empty string means "no
//! modification — leave the input alone."

pub mod pre_tool_use;
pub mod session_start;
pub mod user_prompt_submit;
pub mod pre_compact;
