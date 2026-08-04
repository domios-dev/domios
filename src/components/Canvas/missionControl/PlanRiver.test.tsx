// Run: npx tsx src/components/Canvas/missionControl/PlanRiver.test.tsx

import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { PlanRiver } from "./PlanRiver";
import type { FlowRiverModel } from "./flowModel";
import type { MissionAgent } from "./types";

type TestFn = () => void | Promise<void>;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/plan-river" });
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
  Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
}

async function render(ui: ReactElement) {
  setupDom();
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root: Root = createRoot(host);
  await act(async () => {
    root.render(ui);
  });
  return {
    host,
    unmount: async () => {
      await act(async () => root.unmount());
      host.remove();
    },
  };
}

const VERDI: MissionAgent = { id: "tab-v", name: "Verdi", provider: "claude", state: "reviewing" };

function activeModel(overrides: Partial<FlowRiverModel> = {}): FlowRiverModel {
  return {
    planId: "pl_1",
    planTitle: "Plan views UX redesign",
    progressPct: 88,
    statusLabel: null,
    crew: [VERDI, { id: "tab-p", name: "Puccini", provider: "codex", state: "working" }],
    crewCount: 2,
    frontier: { count: 4, chips: [{ taskId: "f1", title: "Terminal longtasks", hot: false }] },
    milestone: {
      active: true,
      key: "wave-26",
      label: "Wave 26",
      remaining: 2,
      total: 5,
      chips: [{ taskId: "m1", title: "Mobile compact header", hot: true, carrier: VERDI }],
    },
    verify: {
      inReview: [{ taskId: "v1", title: "Soak review", hot: true, carrier: VERDI }],
      gates: [{ id: "g1", question: "Ship compact header?", waitsOn: "Planner", escalated: false }],
      count: 2,
    },
    done: { count: 101, todayDelta: 6, lastStride: "Wave 25 shipped" },
    carrier: { agent: VERDI, label: "soak evidence → gate" },
    hud: { lanesLive: 2, qaQueue: 0, loops: 3, cleanup: 10, next: "Wave 27" },
    isEmpty: false,
    ...overrides,
  };
}

test("renders four flow stations with milestone glowing + N-of-M", async () => {
  const { host, unmount } = await render(<PlanRiver model={activeModel()} />);
  assert(host.querySelector('[data-testid="flow-station-frontier"]'), "frontier station");
  const ms = host.querySelector('[data-testid="flow-station-milestone"]');
  assert(ms, "milestone station");
  assert(ms!.className.includes("mc-flow-station-now"), "milestone glows (now)");
  assert(host.textContent?.includes("Wave 26"), "milestone label");
  assert(host.textContent?.includes("2 of 5 left"), "N-of-M remaining");
  assert(host.querySelector('[data-testid="flow-station-verify"]'), "verify station");
  assert(host.querySelector('[data-testid="flow-station-done"]'), "done station");
  assert(host.textContent?.includes("101"), "done count");
  await unmount();
});

test("F5: verify station shows real in_review chip + decision gate (question + waits-on)", async () => {
  const { host, unmount } = await render(<PlanRiver model={activeModel()} />);
  const verify = host.querySelector('[data-testid="flow-station-verify"]')!;
  assert(verify.querySelector('[data-testid="flow-chip-v1"]'), "in_review chip present");
  const gate = host.querySelector('[data-testid="flow-gate-g1"]');
  assert(gate, "decision gate row present");
  assert(gate!.textContent?.includes("Ship compact header?"), "gate question");
  assert(gate!.textContent?.includes("waits on Planner"), "gate waits-on");
  assert(!host.textContent?.includes("flows on close"), "no jargon placeholder");
  await unmount();
});

test("F1: strides surfaced in Done (last recorded milestone)", async () => {
  const { host, unmount } = await render(<PlanRiver model={activeModel()} />);
  assert(host.textContent?.includes("Wave 25 shipped"), "last stride shown");
  await unmount();
});

test("F1: no ghost stations — verify/done absent when empty of content", async () => {
  const model = activeModel({
    verify: { inReview: [], gates: [], count: 0 },
    done: { count: 0, todayDelta: 0, lastStride: null },
  });
  const { host, unmount } = await render(<PlanRiver model={model} />);
  assert(!host.querySelector('[data-testid="flow-station-verify"]'), "no ghost verify");
  assert(!host.querySelector('[data-testid="flow-station-done"]'), "no ghost done");
  assert(host.querySelector('[data-testid="flow-station-milestone"]'), "milestone still present (active)");
  await unmount();
});

test("F4: empty plan renders head-only compact block (no river band/hud)", async () => {
  const model = activeModel({
    frontier: { count: 0, chips: [] },
    milestone: { active: false, key: null, label: "none active", remaining: 0, total: 0, chips: [] },
    verify: { inReview: [], gates: [], count: 0 },
    done: { count: 0, todayDelta: 0, lastStride: null },
    carrier: null,
    isEmpty: true,
  });
  const { host, unmount } = await render(<PlanRiver model={model} />);
  const river = host.querySelector('[data-testid="plan-river-pl_1"]')!;
  assert(river.getAttribute("data-empty") === "true", "flagged empty");
  assert(!host.querySelector('[data-testid="flow-station-frontier"]'), "no stations");
  assert(!host.querySelector('[data-testid="river-hud-pl_1"]'), "no zero-HUD");
  assert(host.querySelector(".mc-river-band") === null, "no river band");
  assert(host.textContent?.includes("No active work"), "head-only note");
  await unmount();
});

test("milestone fallback renders 'none active' and dims (with inflow)", async () => {
  const model = activeModel({
    milestone: { active: false, key: null, label: "none active", remaining: 0, total: 0, chips: [] },
  });
  const { host, unmount } = await render(<PlanRiver model={model} />);
  const ms = host.querySelector('[data-testid="flow-station-milestone"]');
  assert(ms!.textContent?.includes("none active"), "none-active label");
  assert(ms!.className.includes("mc-flow-station-dim"), "dimmed");
  assert(!ms!.className.includes("mc-flow-station-now"), "not glowing");
  await unmount();
});

test("compact river hides verify/done + hud", async () => {
  const { host, unmount } = await render(<PlanRiver model={activeModel()} compact />);
  assert(host.querySelector('[data-testid="flow-station-frontier"]'), "frontier shown compact");
  assert(!host.querySelector('[data-testid="river-hud-pl_1"]'), "no hud when compact");
  await unmount();
});

test("chip click fires onTaskClick", async () => {
  let clicked: string | null = null;
  const { host, unmount } = await render(<PlanRiver model={activeModel()} onTaskClick={(id) => (clicked = id)} />);
  const chip = host.querySelector('[data-testid="flow-chip-m1"]') as HTMLElement;
  await act(async () => chip.click());
  assert(clicked === "m1", `chip click routes task id, got ${clicked}`);
  await unmount();
});

async function main() {
  let failed = 0;
  for (const [name, fn] of tests) {
    try {
      await fn();
      console.log(`  ✓ ${name}`);
    } catch (e) {
      failed += 1;
      console.error(`  ✗ ${name}: ${(e as Error).message}`);
    }
  }
  if (failed > 0) {
    console.error(`\n${failed} of ${tests.length} PlanRiver test(s) failed`);
    process.exit(1);
  }
  console.log(`\n✓ all ${tests.length} PlanRiver test(s) green`);
}

void main();
