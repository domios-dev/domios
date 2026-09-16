//! The inline-argv boundary for coordination handoffs.
//!
//! A payload typed into a shell command is expanded by that shell before
//! `execve`, so backtick spans, `$` expansions and quote runs are already gone
//! when argv reaches this process. Nothing downstream can detect or recover
//! them, which is why an authored payload is refused here instead of staged:
//! staging post-mangle bytes would manufacture a byte-faithful-looking success.

/// Longest single-line payload still treated as a scalar argument. Above it, or
/// with any newline, the text is authored prose that must arrive through a file.
pub const INLINE_ARGV_BUDGET: usize = 240;

/// Stable discriminator so a caller can tell a refused handoff from a transport
/// failure without parsing prose.
pub const REFUSAL_CODE: &str = "inline_handoff_refused";

/// Source label used by every resolver for a payload that came from argv.
pub const INLINE_SOURCE: &str = "positional";

/// Remedies, one per coordination surface, naming only flags that surface has.
pub const PAD_REMEDY: &str =
    "--from-pad <relative-path> (also accepted: --from-file <path>, or --stdin with a quoted heredoc)";
pub const SPAWN_REMEDY: &str =
    "--task-file <path> (also accepted: --task-file - with a quoted heredoc)";

/// An authored payload: prose a human or agent wrote, not a scalar argument.
pub fn is_authored_payload(text: &str) -> bool {
    text.contains('\n') || text.chars().count() > INLINE_ARGV_BUDGET
}

fn shape_of(text: &str) -> String {
    if text.contains('\n') {
        "multiline".to_string()
    } else {
        format!("over {INLINE_ARGV_BUDGET} characters")
    }
}

/// The refusal an inline authored payload earns, or `None` when the payload
/// either arrived through a staged source or is a scalar argument.
pub fn inline_refusal(source: &str, text: &str, remedy: &str) -> Option<String> {
    if source != INLINE_SOURCE || !is_authored_payload(text) {
        return None;
    }
    Some(format!(
        "{REFUSAL_CODE}: this payload is {} and went inline through the shell, which silently \
         mangles backticks, $, quotes, and newlines. The original bytes are already gone, so it \
         is refused before the handoff is accepted rather than delivered mangled.\n\
         Required: stage the payload in this tab's pad (`codeterm pad path`) and pass {remedy}.",
        shape_of(text)
    ))
}

/// Refuse before any transport, for call sites that cannot return an error.
pub fn refuse_inline(source: &str, text: &str, remedy: &str) {
    if let Some(refusal) = inline_refusal(source, text, remedy) {
        eprintln!("Error: {refusal}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests;
