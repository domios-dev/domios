import { Map, Workflow } from "lucide-react";
import { AgentStack } from "../Canvas/presence/AgentCircle";
import type { OrchestratorPlanSummary } from "../Canvas/paneOwnedPlans";

// EMERGENCE (north-star #2), OUTSIDE the canvas: an ORCHESTRATOR pane wears the
// progress of the plan it drives right above its composer — "how much of the plan
// is left" + who is on it — and one click flies to the plan overview on the board.
// Signature = a determinate progress meter; everything else stays quiet around it.
// The product's live-presence green (matches the plan heartbeat dot on the board).
const LIVE_ACCENT = "rgba(82,180,145,0.95)";

export interface OrchestratorProgressStripProps {
  summary: OrchestratorPlanSummary;
  /** Other plans this pane also owns, collapsed into a "+N plans" tail. */
  extraPlans?: number;
  onOpen?: () => void;
}

export function OrchestratorProgressStrip({ summary, extraPlans = 0, onOpen }: OrchestratorProgressStripProps) {
  const { plan, done, total, pct, activeLanes, live, focus, agents } = summary;
  const complete = total > 0 && done >= total;
  const fill = complete ? "rgba(52,211,153,0.95)" : LIVE_ACCENT;
  const title = `${plan.title} — ${done}/${total} done (${pct}%)${activeLanes ? ` · ${activeLanes} active lane${activeLanes === 1 ? "" : "s"}` : ""}${focus ? `\n${focus}` : ""}\nOpen the plan on the canvas`;
  return (
    <button
      type="button"
      data-testid="orchestrator-progress-strip"
      data-plan-id={plan.id}
      data-plan-live={live ? "true" : "false"}
      data-plan-pct={pct}
      title={title}
      onMouseDown={(e) => e.preventDefault()}
      onClick={onOpen}
      className="mb-2 flex w-full min-w-0 items-center gap-2.5 overflow-hidden rounded-[0.9rem] border py-1 pl-2 pr-2.5 text-left transition hover:brightness-110"
      style={{
        background: "var(--ct-bg-elev, var(--ct-surface, #161b22))",
        borderColor: `color-mix(in srgb, ${LIVE_ACCENT} 38%, var(--ct-border, #30363d))`,
        borderLeft: `3px solid ${LIVE_ACCENT}`,
        cursor: onOpen ? "pointer" : "default",
      }}
    >
      <Workflow
        size={13}
        strokeWidth={2.2}
        aria-hidden
        className={live ? "ct-presence-live shrink-0" : "shrink-0"}
        style={{ color: LIVE_ACCENT, ["--ct-presence-color" as string]: LIVE_ACCENT }}
      />
      <span
        data-testid="orchestrator-progress-strip-title"
        data-plan-title-full={plan.title}
        className="min-w-0 truncate"
        style={{ flex: "1 1 auto", fontSize: "0.78rem", fontWeight: 600, color: "var(--ct-fg, #c9d1d9)" }}
      >
        {plan.title}
      </span>

      <span data-testid="orchestrator-progress-strip-progress" className="flex shrink-0 items-center gap-1.5">
        <span
          aria-hidden
          style={{ width: 56, height: 6, borderRadius: 3, overflow: "hidden", background: "var(--ct-bg-active, #21262d)", flexShrink: 0 }}
        >
          <span
            data-testid="orchestrator-progress-strip-bar"
            style={{ display: "block", width: `${pct}%`, height: "100%", background: fill, transition: "width 240ms var(--ct-ease-decel, ease)" }}
          />
        </span>
        <span
          data-testid="orchestrator-progress-strip-pct"
          style={{ fontSize: "0.66rem", fontWeight: 700, fontVariantNumeric: "tabular-nums", color: "var(--ct-fg, #c9d1d9)", whiteSpace: "nowrap" }}
        >
          {pct}% · {done}/{total}
        </span>
      </span>

      {activeLanes > 0 && (
        <span
          data-testid="orchestrator-progress-strip-lanes"
          title={`${activeLanes} active lane${activeLanes === 1 ? "" : "s"}`}
          className="inline-flex shrink-0 items-center gap-1"
          style={{
            padding: "0.05em 0.5em",
            borderRadius: 999,
            fontSize: "0.62rem",
            fontWeight: 700,
            fontVariantNumeric: "tabular-nums",
            whiteSpace: "nowrap",
            color: LIVE_ACCENT,
            background: `color-mix(in srgb, ${LIVE_ACCENT} 13%, transparent)`,
            border: `1px solid color-mix(in srgb, ${LIVE_ACCENT} 42%, transparent)`,
          }}
        >
          <span className={live ? "ct-canvas-heartbeat-dot" : undefined} aria-hidden style={{ width: 5, height: 5, borderRadius: "50%", background: LIVE_ACCENT, flexShrink: 0 }} />
          {activeLanes} {activeLanes === 1 ? "lane" : "lanes"}
        </span>
      )}

      {focus && (
        <span
          data-testid="orchestrator-progress-strip-focus"
          className="min-w-0 truncate"
          style={{ flex: "1 8 6rem", fontSize: "0.68rem", fontWeight: 500, fontStyle: "italic", color: "color-mix(in srgb, var(--ct-fg, #c9d1d9) 64%, transparent)" }}
        >
          {focus}
        </span>
      )}

      {agents.length > 0 && (
        <span data-testid="orchestrator-progress-strip-faces" className="shrink-0">
          <AgentStack agents={agents} size={16} max={4} live={live} />
        </span>
      )}

      {extraPlans > 0 && (
        <span
          data-testid="orchestrator-progress-strip-more"
          title={`${extraPlans} more plan${extraPlans === 1 ? "" : "s"} owned by this pane`}
          className="shrink-0"
          style={{ fontSize: "0.62rem", fontWeight: 700, whiteSpace: "nowrap", color: "var(--ct-muted-fg, #8b949e)" }}
        >
          +{extraPlans} plan{extraPlans === 1 ? "" : "s"}
        </span>
      )}

      <Map size={12} aria-hidden className="shrink-0 opacity-50" style={{ color: "var(--ct-muted-fg, #8b949e)" }} />
    </button>
  );
}
