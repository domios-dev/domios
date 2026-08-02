//! Fixed injection templates for the refresh cycle. Kept literal-free of any
//! plan-specific data except the `{plan_id}` slot the driver fills.

/// S1 — the request injected to the orchestrator to snapshot its in-head delta
/// before context is compacted. The driver verifies the resulting handoff stride
/// via plan readback; this text only asks for it.
pub const HANDOFF_REQUEST: &str = "\
Refresh starting — snapshot BEFORE compaction. Record a HANDOFF stride capturing \
only the in-head delta not already in plan state: current_focus, in-flight lanes \
+ their expected next events, unjudged reports, and any promises made to the user. \
Run `codeterm orchestrator handoff --plan {plan_id}` (or `codeterm plan record \
{plan_id} --step handoff:<summary>`). Do this now; refresh waits for it.";

/// S4 — the fixed re-orient template injected AFTER compaction settles. The
/// `<<MEM_PREFERENCES>>` marker is the single injection point for the
/// mem-preferences block owned by task_5939a153 — do NOT build that block here.
pub const REORIENT: &str = "\
Refresh complete. Re-orient before resuming:
(1) `codeterm plan state {plan_id}` — re-read the live frontier.
(2) Read your latest HANDOFF stride (`codeterm plan handoff show {plan_id} --latest --markdown`).
(3) If role rules feel fuzzy: `codeterm docs ORCHESTRATION --section the-loop`.
(4) Re-derive the frontier and set current_focus.
<<MEM_PREFERENCES>>
Resume work now.";

/// Render a template, substituting the plan id and stripping the unfilled
/// mem-preferences marker (task_5939a153 replaces it at its own injection point).
pub fn render(template: &str, plan_id: &str) -> String {
    template
        .replace("{plan_id}", plan_id)
        .replace("<<MEM_PREFERENCES>>\n", "")
        .replace("<<MEM_PREFERENCES>>", "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_fills_plan_id_everywhere() {
        let out = render(REORIENT, "pl_42");
        assert!(out.contains("codeterm plan state pl_42"));
        assert!(out.contains("codeterm plan handoff show pl_42"));
        assert!(!out.contains("{plan_id}"));
    }

    #[test]
    fn render_strips_unfilled_mem_marker() {
        let out = render(REORIENT, "pl_1");
        assert!(!out.contains("<<MEM_PREFERENCES>>"));
        // The marker's removal must not weld the surrounding lines together.
        assert!(out.contains("set current_focus.\nResume work now."));
    }

    #[test]
    fn handoff_request_carries_plan_id() {
        let out = render(HANDOFF_REQUEST, "pl_7");
        assert!(out.contains("--plan pl_7"));
        assert!(out.contains("plan record pl_7"));
    }
}
