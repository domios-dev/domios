// Run: npx tsx src/components/Terminal/OrchestratorProgressStrip.test.tsx
// EMERGENCE (north-star #2), orchestrator side. A live PTY pane is not reliably
// web-mode drivable, so the strip's behaviour is locked here as a component test:
// an owner pane gets the progress strip (bar/%/lanes/focus/faces + fly-to-plan
// click); a non-owner falls through; ranking picks the most active plan.

import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { OrchestratorProgressStrip } from "./OrchestratorProgressStrip";
import { resolvePaneOwnedPlans } from "../Canvas/paneOwnedPlans";
import type { Pane } from "../../lib/types";
import type { Plan, Task, TaskStatus } from "../Canvas/types";

type TestFn = () => void | Promise<void>;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const emptyCanvas = { x: null, y: null, collapsed: false };

function mkTask(id: string, status: TaskStatus, extra: Partial<Task> = {}): Task {
  return {
    id,
    name: `Task ${id}`,
    type: "implement",
    model: null,
    model_rationale: null,
    status,
    status_label: null,
    section_ids: [],
    body: "",
    intent: null,
    parent_task_id: null,
    assigned_pane_id: null,
    peer: null,
    canvas: emptyCanvas,
    milestones: [],
    ...extra,
  };
}

function mkPlan(over: Partial<Plan> = {}): Plan {
  return {
    id: "plan-1",
    title: "Canvas rendering substrate — Figma-grade migration",
    sections: [],
    status: "active",
    owner: { pane_id: "orch-1", session_id: "sess-orch", title: "Orchestrator" },
    project_path: "/tmp/p",
    created_at: 1,
    updated_at: 100,
    milestone_kinds: [],
    loops: [],
    elements: [],
    tasks: [],
    canvas: { viewport: null },
    owner_history: [],
    ...over,
  };
}

function mkPane(id: string, over: Partial<Pane> = {}): Pane {
  return { id, pty_id: null, pane_type: "Terminal", title: id, cwd: null, ...over } as Pane;
}

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/orch-strip" });
  (window as unknown as { __TAURI_INTERNALS__: Record<string, unknown> }).__TAURI_INTERNALS__ = {};
  for (const k of ["window", "document", "navigator", "HTMLElement", "HTMLButtonElement", "Node", "Event", "CustomEvent"] as const) {
    Object.defineProperty(globalThis, k, { value: (window as never)[k], configurable: true });
  }
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
}

async function render(ui: ReactElement): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => { root.render(ui); });
  await act(async () => { await Promise.resolve(); });
  return { host, root };
}

// ── data: resolvePaneOwnedPlans ───────────────────────────────────────────────
test("owner pane resolves its plan; progress + lanes + focus + agents computed", () => {
  const plan = mkPlan({
    heartbeat: { current_focus: "Wiring the camera matrix renderer", active_lanes: ["feat/a", "feat/b"] },
    tasks: [
      mkTask("t1", "done"),
      mkTask("t2", "done"),
      mkTask("t3", "in_progress", { assigned_pane_id: "w1", taken_by_role: "frontend-dev" }),
      mkTask("t4", "pending"),
      mkTask("t5", "cancelled"),
    ],
  });
  const panes = { "orch-1": mkPane("orch-1", { agent_session_id: "sess-orch", can_spawn: true }) };
  const [s] = resolvePaneOwnedPlans([plan], "orch-1", panes);
  assert(s, "owner pane resolves the plan it owns");
  assert(s.total === 4, `cancelled excluded from total (got ${s.total})`);
  assert(s.done === 2, `done counts done tasks (got ${s.done})`);
  assert(s.pct === 50, `pct = round(2/4) = 50 (got ${s.pct})`);
  assert(s.activeLanes === 2, `lanes from heartbeat (got ${s.activeLanes})`);
  assert(s.live === true, "in_progress task → live");
  assert(s.focus === "Wiring the camera matrix renderer", "focus from heartbeat");
  assert(s.agents.length >= 1, `at least the lead executor (got ${s.agents.length})`);
});

test("non-owner pane (and child worker) resolves NO owned plan", () => {
  const plan = mkPlan();
  const panes = {
    "orch-1": mkPane("orch-1", { agent_session_id: "sess-orch" }),
    w1: mkPane("w1", { parent_pane_id: "orch-1", agent_session_id: "sess-w1" }),
  };
  assert(resolvePaneOwnedPlans([plan], "w1", panes).length === 0, "child worker does NOT inherit ownership");
  assert(resolvePaneOwnedPlans([plan], "stranger", {}).length === 0, "unrelated pane owns nothing");
});

test("session-id ownership survives a pane-id change", () => {
  const plan = mkPlan({ owner: { pane_id: "old-pane", session_id: "sess-orch", title: "Orchestrator" } });
  const panes = { "orch-2": mkPane("orch-2", { agent_session_id: "sess-orch" }) };
  assert(resolvePaneOwnedPlans([plan], "orch-2", panes).length === 1, "matches on the pane's own session id");
});

test("multi-plan owner ranks the most active plan first", () => {
  const quiet = mkPlan({ id: "quiet", title: "Quiet", updated_at: 999, tasks: [mkTask("a", "pending")] });
  const busy = mkPlan({ id: "busy", title: "Busy", updated_at: 1, tasks: [mkTask("b", "in_progress", { assigned_pane_id: "w1" })] });
  const panes = { "orch-1": mkPane("orch-1", { agent_session_id: "sess-orch" }) };
  const ranked = resolvePaneOwnedPlans([quiet, busy], "orch-1", panes);
  assert(ranked.length === 2, "both owned plans returned");
  assert(ranked[0].plan.id === "busy", `live plan leads despite older updated_at (got ${ranked[0].plan.id})`);
});

// ── view: OrchestratorProgressStrip ───────────────────────────────────────────
test("strip renders bar/%/lanes/focus/faces and fires fly-to-plan on click", async () => {
  const plan = mkPlan({
    heartbeat: { current_focus: "Wiring the camera matrix renderer", active_lanes: ["feat/a"] },
    tasks: [mkTask("t1", "done"), mkTask("t2", "in_progress", { assigned_pane_id: "w1", taken_by_role: "frontend-dev" })],
  });
  const [s] = resolvePaneOwnedPlans([plan], "orch-1", { "orch-1": mkPane("orch-1", { agent_session_id: "sess-orch" }) });
  let opened = 0;
  const { host, root } = await render(<OrchestratorProgressStrip summary={s} extraPlans={2} onOpen={() => { opened += 1; }} />);
  const strip = host.querySelector('[data-testid="orchestrator-progress-strip"]') as HTMLButtonElement | null;
  assert(strip, "strip renders");
  assert(host.querySelector('[data-testid="orchestrator-progress-strip-bar"]'), "progress bar present");
  const pct = host.querySelector('[data-testid="orchestrator-progress-strip-pct"]');
  assert(pct?.textContent?.includes("50%") && pct.textContent.includes("1/2"), `pct + done/total (got "${pct?.textContent}")`);
  assert(host.querySelector('[data-testid="orchestrator-progress-strip-lanes"]')?.textContent?.includes("1"), "lanes chip");
  assert(host.querySelector('[data-testid="orchestrator-progress-strip-focus"]')?.textContent?.includes("camera matrix"), "focus narrative");
  assert(host.querySelector('[data-testid="orchestrator-progress-strip-faces"]'), "swarm face-stack");
  assert(host.querySelector('[data-testid="orchestrator-progress-strip-more"]')?.textContent?.includes("+2 plans"), "extra-plans tail");
  assert(host.querySelector('[data-testid="orchestrator-progress-strip-bar"]')?.getAttribute("style")?.includes("width: 50%"), "bar width tracks pct");
  await act(async () => { strip!.click(); });
  assert(opened === 1, "click flies to the plan");
  await act(async () => root.unmount());
});

test("empty plan → 0% bar, no lanes/focus, no NaN", async () => {
  const [s] = resolvePaneOwnedPlans([mkPlan({ tasks: [] })], "orch-1", { "orch-1": mkPane("orch-1", { agent_session_id: "sess-orch" }) });
  assert(s.pct === 0 && s.total === 0, "no tasks → 0% of 0");
  const { host, root } = await render(<OrchestratorProgressStrip summary={s} />);
  assert(host.querySelector('[data-testid="orchestrator-progress-strip-pct"]')?.textContent?.includes("0%"), "0% shown, not NaN");
  assert(!host.querySelector('[data-testid="orchestrator-progress-strip-lanes"]'), "no lanes chip when 0 lanes");
  assert(!host.querySelector('[data-testid="orchestrator-progress-strip-focus"]'), "no focus when heartbeat absent");
  await act(async () => root.unmount());
});

async function main() {
  setupDom();
  let failed = 0;
  for (const [name, fn] of tests) {
    try {
      await fn();
      console.log(`  ✓ ${name}`);
    } catch (e) {
      failed++;
      console.error(`  ✗ ${name}`);
      console.error(`    ${e instanceof Error ? e.message : e}`);
    }
  }
  console.log(`\n${tests.length - failed} passed, ${failed} failed`);
  if (failed > 0) process.exit(1);
}

void main();
