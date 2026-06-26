import { Repeat } from "lucide-react";
import type { LoopSpec } from "../../types";
import { atLeast, type World } from "../../worlds";
import { LOOP_STATUS } from "./shared";

function LoopPill({ loop, showDetail }: { loop: LoopSpec; showDetail: boolean }) {
  const meta = LOOP_STATUS[loop.status] ?? LOOP_STATUS.stopped;
  const stages = showDetail ? loop.stages.map((s) => s.role || s.key).filter(Boolean) : [];
  const title = [
    `Loop: ${loop.target}`,
    `Status: ${meta.label}`,
    `Iterations: ${loop.count}${loop.budget > 0 ? ` / ${loop.budget}` : ""}`,
    loop.stop_when ? `Stop when: ${loop.stop_when}` : null,
    stages.length ? `Stages: ${stages.join(" › ")}` : null,
  ].filter(Boolean).join("\n");
  return (
    <span
      data-testid="canvas-plan-loop"
      data-loop-status={loop.status}
      title={title}
      style={{ display: "inline-flex", alignItems: "center", gap: 6, maxWidth: showDetail ? 300 : 210, padding: "0.12em 0.55em", borderRadius: 999, fontSize: "0.76em", border: `1px solid color-mix(in srgb, ${meta.color} 45%, transparent)`, background: `color-mix(in srgb, ${meta.color} 12%, transparent)`, color: "var(--ct-fg)", whiteSpace: "nowrap" }}
    >
      <span className={meta.live ? "ct-canvas-heartbeat-dot" : undefined} aria-hidden style={{ width: 7, height: 7, borderRadius: "50%", background: meta.color, flexShrink: 0 }} />
      <span style={{ minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", fontWeight: 600 }}>{loop.target}</span>
      {loop.budget > 0 && (
        <span data-testid="canvas-plan-loop-count" style={{ fontFamily: "var(--ct-font-mono)", fontSize: "0.92em", opacity: 0.8, flexShrink: 0 }}>
          {loop.count}/{loop.budget}
        </span>
      )}
      {stages.length > 0 && (
        <span style={{ minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", opacity: 0.6, fontSize: "0.9em" }}>{stages.join(" › ")}</span>
      )}
    </span>
  );
}

// §loops: surface the orchestrator's iteration engines. W0 = a one-line count/running
// summary (never a hollow strip); W1+ = a pill per loop (target · count/budget · status
// dot), revealing pipeline stages + stop-when at W2.
export function LoopsStrip({ loops, world }: { loops: LoopSpec[]; world: World }) {
  const running = loops.filter((l) => l.status === "running").length;
  const showDetail = atLeast(world, "W2");
  const maxLoops = 4;

  if (!atLeast(world, "W1")) {
    return (
      <div data-testid="canvas-plan-loops" style={{ flexShrink: 0, padding: "5px 12px", borderBottom: "1px solid var(--ct-border-subtle)", display: "flex", alignItems: "center", gap: 6, fontSize: "0.74em", fontWeight: 600 }}>
        <Repeat size={11} strokeWidth={2.2} style={{ opacity: 0.7, flexShrink: 0 }} aria-hidden />
        <span>{loops.length} loop{loops.length === 1 ? "" : "s"}</span>
        {running > 0 && <span style={{ color: LOOP_STATUS.running.color }}>· {running} running</span>}
      </div>
    );
  }
  return (
    <div data-testid="canvas-plan-loops" className="ct-canvas-fade-in" style={{ flexShrink: 0, padding: "6px 12px", borderBottom: "1px solid var(--ct-border-subtle)", display: "flex", flexWrap: "wrap", alignItems: "center", gap: 6 }}>
      <Repeat size={12} strokeWidth={2.2} style={{ opacity: 0.6, flexShrink: 0 }} aria-hidden />
      {loops.slice(0, maxLoops).map((loop) => (
        <LoopPill key={loop.id} loop={loop} showDetail={showDetail} />
      ))}
      {loops.length > maxLoops && <span style={{ fontSize: "0.72em", opacity: 0.6, alignSelf: "center" }}>+{loops.length - maxLoops} more</span>}
    </div>
  );
}
