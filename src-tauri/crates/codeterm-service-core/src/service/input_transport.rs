//! How a tab receives input, and whether a sender marker belongs in it.

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TabInputTransport {
    Pty,
    ChatBackend,
}

/// Whether to inject the sender return-address marker into the PTY text. The
/// marker is meaningful only to an AGENT reading its input; a bare shell runs
/// the bytes verbatim and a chatBackend carries identity structurally.
pub fn should_prepend_source(
    has_source: bool,
    target_has_agent: bool,
    transport: TabInputTransport,
) -> bool {
    has_source && target_has_agent && transport == TabInputTransport::Pty
}
