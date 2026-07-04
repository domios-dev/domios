import { useState } from "react";
import { PlanRiver } from "./PlanRiver";
import type { FlowRiverModel } from "./flowModel";
import type { MissionAgent } from "./types";

// Deterministic design-reference render of the flow canvas (matches
// mission-control-v2-mockup.html). Data is fixture-only — the live surface
// derives the same shape from buildMissionControl + task groups.

const PUCCINI: MissionAgent = { id: "pane-p", name: "Puccini", provider: "codex", state: "working" };
const VERDI: MissionAgent = { id: "pane-v", name: "Verdi", provider: "claude", state: "working" };
const MOZART: MissionAgent = { id: "pane-m", name: "Mozart", provider: "opencode", state: "working" };
const BACH: MissionAgent = { id: "pane-b", name: "Bach", provider: "claude", state: "working" };
const LISZT: MissionAgent = { id: "pane-l", name: "Liszt", provider: "codex", state: "working" };

const FULL: FlowRiverModel = {
  planId: "pl_ux",
  planTitle: "Plan views UX redesign",
  progressPct: 88,
  statusLabel: null,
  crew: [PUCCINI, VERDI, MOZART, BACH, LISZT],
  crewCount: 5,
  frontier: {
    count: 4,
    chips: [
      { taskId: "f1", title: "Terminal: xterm longtasks", hot: false },
      { taskId: "f2", title: "Chat status parity", hot: false },
    ],
  },
  milestone: {
    active: true,
    key: "wave-26",
    label: "Wave 26",
    remaining: 2,
    total: 5,
    chips: [
      { taskId: "m1", title: "Mobile compact header", hot: true, carrier: VERDI },
      { taskId: "m2", title: "Responsiveness soak", hot: true },
    ],
  },
  verify: { queue: [], judging: [], count: 0 },
  done: { count: 101, todayDelta: 6 },
  carrier: { agent: VERDI, label: "soak evidence → gate" },
  hud: { lanesLive: 2, qaQueue: 0, loops: 3, cleanup: 10, next: "Wave 27 — design system" },
};

const MINI: FlowRiverModel = {
  planId: "pl_infra",
  planTitle: "Build infra experiments",
  progressPct: 0,
  statusLabel: "draft",
  crew: [],
  crewCount: 0,
  frontier: { count: 1, chips: [{ taskId: "i1", title: "Shared sccache over WebDAV", hot: false }] },
  milestone: { active: false, key: null, label: "none active", remaining: 0, total: 0, chips: [] },
  verify: { queue: [], judging: [], count: 0 },
  done: { count: 0, todayDelta: 0 },
  carrier: null,
  hud: { lanesLive: 0, qaQueue: 0, loops: 0, cleanup: 0, next: null },
};

export function FlowCanvasPreview() {
  const [allPlans, setAllPlans] = useState(true);
  const [reducedMotion, setReducedMotion] = useState(false);
  return (
    <div
      className={`mc-root mc-flow-shell${reducedMotion ? " mc-reduced-motion" : ""}`}
      data-testid="flow-canvas-preview"
      style={{ height: "100vh", display: "flex", flexDirection: "column" }}
    >
      <header className="mc-live-toolbar">
        <div className="mc-live-zoom">
          <button type="button" className="mc-pill" onClick={() => setReducedMotion((v) => !v)}>
            {reducedMotion ? "motion off" : "motion on"}
          </button>
        </div>
        <div className="mc-scope-toggle" role="group" aria-label="Plan scope">
          <button
            type="button"
            className="mc-scope-btn"
            data-active={!allPlans ? "true" : undefined}
            onClick={() => setAllPlans(false)}
          >
            workspace
          </button>
          <button
            type="button"
            className="mc-scope-btn"
            data-active={allPlans ? "true" : undefined}
            onClick={() => setAllPlans(true)}
          >
            all plans
          </button>
          <span style={{ color: "var(--mc-muted)" }}>⚙</span>
        </div>
      </header>
      <div className="mc-live-viewport" style={{ overflow: "auto" }}>
        <div className="mc-flow-stack">
          <PlanRiver model={FULL} reducedMotion={reducedMotion} />
          <PlanRiver model={MINI} compact reducedMotion={reducedMotion} />
        </div>
      </div>
    </div>
  );
}
