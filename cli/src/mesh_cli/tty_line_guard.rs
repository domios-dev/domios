//! A Linux/macOS tty in canonical mode truncates a line beyond 4095 bytes while
//! the write still succeeds. A raw-shell send over the mesh refuses such a line
//! up front instead of reporting ok for data that was cut.

use super::relay::{mesh_relay_invoke_result, MeshSpawnFailure};

pub(crate) const TTY_LINE_LIMIT_BYTES: usize = 4095;
pub(crate) const LINE_EXCEEDS_TTY_LIMIT: &str = "line_exceeds_tty_limit";

pub(crate) fn longest_line_bytes(text: &str) -> usize {
    text.split('\n')
        .map(|line| line.trim_end_matches('\r').len())
        .max()
        .unwrap_or(0)
}

/// Windows ConPTY delivers arbitrarily long lines; every other platform has the tty limit.
pub(crate) fn platform_has_tty_line_limit(platform: &str) -> bool {
    !platform.eq_ignore_ascii_case("windows")
}

pub(crate) fn refusal(peer: &str, longest: usize) -> MeshSpawnFailure {
    MeshSpawnFailure::refused_before_send(
        peer,
        LINE_EXCEEDS_TTY_LIMIT,
        format!(
            "a line of {longest} bytes exceeds the {TTY_LINE_LIMIT_BYTES}-byte tty line limit of the \
             peer's platform and would be silently truncated"
        ),
        "split the payload into lines of at most 4095 bytes, or deliver it as a file the peer reads",
    )
}

/// Refuse (and exit) when a `--to-shell` payload has an over-long line and the
/// peer's platform truncates it. An unreachable identify leaves the send to fail
/// on its own path.
pub(super) fn enforce(target: &str, body: Option<&str>, json_mode: bool) {
    let Some(text) = body
        .and_then(|body| serde_json::from_str::<serde_json::Value>(body).ok())
        .and_then(|body| {
            body.get("text")
                .and_then(|t| t.as_str())
                .map(str::to_string)
        })
    else {
        return;
    };
    let longest = longest_line_bytes(&text);
    if longest <= TTY_LINE_LIMIT_BYTES {
        return;
    }
    let platform = mesh_relay_invoke_result(target, "GET", "/api/identify", None)
        .ok()
        .and_then(|identify| {
            identify
                .get("platform")
                .and_then(|p| p.as_str())
                .map(str::to_string)
        });
    if platform.is_some_and(|p| platform_has_tty_line_limit(&p)) {
        refusal(target, longest).print_and_exit(json_mode);
    }
}
