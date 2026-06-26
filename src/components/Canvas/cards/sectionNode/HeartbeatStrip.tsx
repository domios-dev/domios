import { Activity, GitBranch } from "lucide-react";
import type { OrchestratorHeartbeat } from "../../types";
import { atLeast, type World } from "../../worlds";
import { HEARTBEAT_ACCENT } from "./shared";

// §heartbeat: the orchestrator's PRESENT tense — current focus + the lanes it is
// driving right now, with operating rules (W2+) and recent learnings (W3) below.
// The goal strip above is the destination; this strip is the live pulse toward it.
export function HeartbeatStrip({ hb, world }: { hb: OrchestratorHeartbeat; world: World }) {
  const focus = hb.current_focus?.trim() ?? "";
  const lanes = (hb.active_lanes ?? []).map((l) => l.trim()).filter(Boolean);
  const rules = atLeast(world, "W2") ? (hb.rules ?? []).map((r) => r.trim()).filter(Boolean) : [];
  const learnings = atLeast(world, "W3") ? (hb.learnings ?? []).filter((l) => l.text?.trim()) : [];
  const showLanes = atLeast(world, "W1") && lanes.length > 0;
  const maxLanes = 5;

  return (
    <div
      data-testid="canvas-plan-heartbeat"
      className="ct-canvas-fade-in"
      style={{ flexShrink: 0, padding: "6px 12px", borderBottom: "1px solid var(--ct-border-subtle)", background: `color-mix(in srgb, ${HEARTBEAT_ACCENT} 7%, transparent)`, display: "flex", flexDirection: "column", gap: 5 }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 7, minWidth: 0 }}>
        <span aria-hidden style={{ display: "inline-flex", alignItems: "center", flexShrink: 0, color: HEARTBEAT_ACCENT }}>
          <span className="ct-canvas-heartbeat-dot" style={{ width: 7, height: 7, borderRadius: "50%", background: HEARTBEAT_ACCENT, marginRight: 5 }} />
          <Activity size={12} strokeWidth={2.4} />
        </span>
        <span style={{ fontSize: "0.62em", fontWeight: 800, letterSpacing: "0.14em", textTransform: "uppercase", color: HEARTBEAT_ACCENT, flexShrink: 0 }}>Now</span>
        {focus && (
          <span
            data-testid="canvas-plan-focus"
            title={focus}
            data-canvas-selectable
            className="ct-canvas-text"
            style={{ flex: 1, minWidth: 0, fontSize: "0.82em", fontWeight: 600, lineHeight: 1.3, display: "-webkit-box", WebkitLineClamp: atLeast(world, "W1") ? 2 : 1, WebkitBoxOrient: "vertical", overflow: "hidden" }}
          >
            {focus}
          </span>
        )}
      </div>
      {showLanes && (
        <div data-testid="canvas-plan-lanes" style={{ display: "flex", flexWrap: "wrap", gap: 5, paddingLeft: 1 }}>
          {lanes.slice(0, maxLanes).map((lane) => (
            <span
              key={lane}
              data-testid="canvas-plan-lane"
              title={lane}
              style={{ display: "inline-flex", alignItems: "center", gap: 4, maxWidth: 160, fontSize: "0.72em", fontWeight: 600, padding: "0.05em 0.5em", borderRadius: 999, border: `1px solid color-mix(in srgb, ${HEARTBEAT_ACCENT} 45%, transparent)`, background: `color-mix(in srgb, ${HEARTBEAT_ACCENT} 13%, transparent)`, color: "var(--ct-fg)", whiteSpace: "nowrap" }}
            >
              <GitBranch size={10} aria-hidden style={{ flexShrink: 0, opacity: 0.8 }} />
              <span style={{ minWidth: 0, overflow: "hidden", textOverflow: "ellipsis" }}>{lane}</span>
            </span>
          ))}
          {lanes.length > maxLanes && <span style={{ fontSize: "0.7em", opacity: 0.6, alignSelf: "center" }}>+{lanes.length - maxLanes}</span>}
        </div>
      )}
      {rules.length > 0 && (
        <ul data-testid="canvas-plan-rules" style={{ margin: 0, paddingLeft: 16, fontSize: "0.76em", lineHeight: 1.4, opacity: 0.92, display: "flex", flexDirection: "column", gap: 2 }}>
          {rules.slice(0, 4).map((r, i) => (
            <li key={i} style={{ overflowWrap: "anywhere" }}>{r}</li>
          ))}
          {rules.length > 4 && <li style={{ listStyle: "none", opacity: 0.6 }}>+{rules.length - 4} more</li>}
        </ul>
      )}
      {learnings.length > 0 && (
        <div data-testid="canvas-plan-learnings" style={{ display: "flex", flexDirection: "column", gap: 3, marginTop: 1 }}>
          {learnings.slice(-3).reverse().map((l) => (
            <div key={l.id} style={{ display: "flex", gap: 5, fontSize: "0.75em", lineHeight: 1.35 }}>
              <span aria-hidden style={{ flexShrink: 0, opacity: 0.7 }}>💡</span>
              <span style={{ minWidth: 0, overflowWrap: "anywhere" }}>
                {l.tag && <span style={{ fontWeight: 700, opacity: 0.8, marginRight: 4 }}>{l.tag}</span>}
                {l.text}
              </span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
