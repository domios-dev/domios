import { AgentStateIndicator, type AgentState } from "./AgentStateIndicator";

// Storybook-style render of every agent state + question subtype, on the chat
// canvas background. Not wired into the app shell — mount it in a dev route or
// screenshot it standalone to review the design. Pairs with the static mirror
// kept for visual review.
const ROWS: Array<{ label: string; state: AgentState }> = [
  { label: "working", state: { state: "working" } },
  {
    label: "working · thinking",
    state: {
      state: "working",
      phase: "thinking",
      status_line: "Swooping… (6m 24s · ↓ 29.8k tokens · still thinking with high effort)",
    },
  },
  {
    label: "working · streaming",
    state: {
      state: "working",
      phase: "streaming",
      status_line: "Composing the answer… (18s · ↑ 2.1k tokens)",
    },
  },
  { label: "idle — your turn", state: { state: "idle" } },
  { label: "idle · summary", state: { state: "idle", last_turn_summary: "Churned for 1m 37s" } },
  { label: "idle — long (resting)", state: { state: "idle", longIdle: true } },
  { label: "question · model_choice", state: { state: "question", subtype: "model_choice" } },
  { label: "question · model_picker", state: { state: "question", subtype: "model_picker" } },
  { label: "question · permission", state: { state: "question", subtype: "permission" } },
  { label: "question · auth", state: { state: "question", subtype: "auth" } },
  { label: "question · rate_limit", state: { state: "question", subtype: "rate_limit" } },
  { label: "question · update", state: { state: "question", subtype: "update" } },
  { label: "question · error", state: { state: "question", subtype: "error" } },
  { label: "question · unknown", state: { state: "question", subtype: "unknown" } },
];

export function AgentStateGallery() {
  return (
    <div
      style={{
        background: "var(--ct-bg, #0d1117)",
        color: "var(--ct-fg, #c9d1d9)",
        fontFamily: "var(--ct-font-ui)",
        padding: "28px",
        display: "grid",
        gridTemplateColumns: "minmax(160px, max-content) 1fr",
        gap: "6px 28px",
        alignItems: "center",
        maxWidth: "720px",
      }}
    >
      {ROWS.map((row) => (
        <div key={row.label} style={{ display: "contents" }}>
          <span
            style={{
              fontFamily: "var(--ct-font-mono)",
              fontSize: "11px",
              color: "var(--ct-muted-fg, #8b949e)",
              textAlign: "right",
            }}
          >
            {row.label}
          </span>
          <AgentStateIndicator state={row.state} />
        </div>
      ))}
    </div>
  );
}
