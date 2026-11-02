//! Fixed injection templates for the refresh cycle. Kept literal-free of any
//! plan-specific data except the `{plan_id}` slot the driver fills.

/// S1 — the request injected to the orchestrator to snapshot its in-head delta
/// before context is compacted. The driver verifies the resulting handoff stride
/// via plan readback; this text only asks for it.
pub const HANDOFF_REQUEST: &str = "\
Refresh starting — snapshot BEFORE compaction. Record a HANDOFF stride capturing \
only the in-head delta not already in plan state: current_focus, in-flight lanes \
+ their expected next events, unjudged reports, and any promises made to the user. \
Run `domios orchestrator handoff --plan {plan_id}` (or `domios plan record \
{plan_id} --step handoff:<summary>`). Do this now; refresh waits for it.";

/// S4 — the fixed re-orient template injected AFTER compaction settles. The
/// `<<MEM_PREFERENCES>>` marker is the single injection point for the
/// mem-preferences block owned by task_5939a153 — do NOT build that block here.
pub const REORIENT: &str = "\
Refresh complete. Re-orient before resuming:
(1) `domios plan state {plan_id}` — re-read the live frontier.
(2) Read your latest HANDOFF stride (`domios plan handoff show {plan_id} --latest --markdown`).
(3) If role rules feel fuzzy: `domios docs ORCHESTRATION`.
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
        let out = render("first<{plan_id}> second<{plan_id}>", "opaque-plan");
        assert_eq!(out, "first<opaque-plan> second<opaque-plan>");
    }

    #[test]
    fn render_strips_unfilled_mem_marker() {
        let out = render("before\n<<MEM_PREFERENCES>>\nafter", "opaque-plan");
        assert_eq!(out, "before\nafter");
    }
}
