//! All agent-facing prompt templates in one place.
//!
//! Edit these strings to change what instructions agents receive.
//! No recompilation needed if we later move to an external file.

/// Appended to Worker tasks — tells them how to report back.
pub const WORKER_SUFFIX: &str = "\nWhen done, report by running: codeterm agent report \"your summary\"";

/// Appended to Lead tasks — spawn instructions + no-sleep rule.
pub const LEAD_SUFFIX: &str = concat!(
    "\nWhen spawning workers, include in --task: 'then run: codeterm agent report your-summary'.",
    "\nIMPORTANT: Do NOT use sleep or poll for reports.",
    " Reports from your workers are auto-injected into your terminal as [Report from ...] messages.",
    " Continue your work — you will see them when they arrive.",
    " After all expected reports arrive, run: codeterm agent report \"your summary\""
);

/// Appended to Orchestrator tasks — no-sleep rule.
pub const ORCHESTRATOR_SUFFIX: &str = concat!(
    "\nIMPORTANT: Do NOT use sleep or poll for reports.",
    " Reports from your agents are auto-injected into your terminal as [Report from ...] messages.",
    " Continue your work — you will see them when they arrive."
);

/// Appended to cron tasks when creator_pane_id is set.
pub const CRON_REPORT_HINT: &str =
    "\n\nYou can report progress back to the cron creator by running: codeterm agent report \"your message\"";

/// Appended to every cron task — completion instruction. `{}` is replaced with run_id.
pub const CRON_COMPLETE_SUFFIX: &str = concat!(
    "\n\nIMPORTANT: When you are completely done with this task,",
    " you MUST run this exact command as your FINAL action:\n",
    "codeterm cron complete --run-id {} --summary \"<your concise summary of what happened and the result>\""
);
