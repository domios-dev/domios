import { useState } from "react";
import { PlanRiver } from "./PlanRiver";
import type { FlowRiverModel } from "./flowModel";
import type { MissionAgent } from "./types";

// Deterministic design-reference render of the flow canvas (matches
// mission-control-v2-mockup.html). Data is fixture-only — the live surface

const PUCCINI: MissionAgent = { id: "tab-p", name: "Puccini", provider: "codex", state: "working" };
const VERDI: MissionAgent = { id: "tab-v", name: "Verdi", provider: "claude", state: "working" };
const MOZART: MissionAgent = { id: "tab-m", name: "Mozart", provider: "opencode", state: "working" };
const BACH: MissionAgent = { id: "tab-b", name: "Bach", provider: "claude", state: "working" };
const LISZT: MissionAgent = { id: "tab-l", name: "Liszt", provider: "codex", state: "working" };

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
  verify: {
    inReview: [{ taskId: "v1", title: "Soak evidence review", hot: true, carrier: VERDI }],
    gates: [
      { id: "g1", question: "Ship compact header to mobile default?", waitsOn: "Planner", escalated: false },
    ],
    count: 2,
  },
  done: { count: 101, todayDelta: 6, lastStride: "Wave 25 shipped — plan views redesign" },
  carrier: { agent: VERDI, label: "soak evidence → gate" },
  hud: { lanesLive: 2, qaQueue: 0, loops: 3, cleanup: 10, next: "Wave 27 — design system" },
  isEmpty: false,
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
  verify: { inReview: [], gates: [], count: 0 },
  done: { count: 0, todayDelta: 0, lastStride: null },
  carrier: null,
  hud: { lanesLive: 0, qaQueue: 0, loops: 0, cleanup: 0, next: null },
  isEmpty: false,
};

// F4: a genuinely empty plan renders as a head-only compact block.
const EMPTY: FlowRiverModel = {
  planId: "pl_empty",
  planTitle: "Archived spike (no active work)",
  progressPct: 100,
  statusLabel: null,
  crew: [],
  crewCount: 0,
  frontier: { count: 0, chips: [] },
  milestone: { active: false, key: null, label: "none active", remaining: 0, total: 0, chips: [] },
  verify: { inReview: [], gates: [], count: 0 },
  done: { count: 0, todayDelta: 0, lastStride: null },
  carrier: null,
  hud: { lanesLive: 0, qaQueue: 0, loops: 0, cleanup: 0, next: null },
  isEmpty: true,
};

export function FlowCanvasPreview() {
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
      </header>
      <div className="mc-live-viewport" style={{ overflow: "auto" }}>
        <div className="mc-flow-stack" style={{ maxWidth: 1240, margin: "0 auto" }}>
          <PlanRiver model={FULL} reducedMotion={reducedMotion} />
          <PlanRiver model={MINI} compact reducedMotion={reducedMotion} />
          <PlanRiver model={EMPTY} compact reducedMotion={reducedMotion} />
        </div>
      </div>
    </div>
  );
}
