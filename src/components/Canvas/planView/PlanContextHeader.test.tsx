// Run: npx tsx src/components/Canvas/planView/PlanContextHeader.test.tsx

import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import type { BoardPlanSummary } from "./boardModel";
import { deriveBoardPlanSummary } from "./boardModel";
import { FIXTURE_PLAN } from "../cards/fixtures.testdata";
import type { Plan } from "../types";
import type { ApiConfig } from "../../../lib/types";
import {
  PlanContextCompactHeader,
  PlanContextHeader,
  PlanContextWideHeader,
} from "./PlanContextHeader";
import { readHeaderCollapsed, writeHeaderCollapsed } from "./usePlanHeaderCollapse";

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
  const window = new Window({ url: "http://localhost/plan-header" });
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "localStorage", { value: window.localStorage, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
}

setupDom();

function setMatchMedia(matchesByQuery: Record<string, boolean>) {
  window.matchMedia = (query: string) => {
    const listeners = new Set<(event: MediaQueryListEvent) => void>();
    return {
      matches: Boolean(matchesByQuery[query]),
      media: query,
      onchange: null,
      addEventListener: (_type: string, listener: (event: MediaQueryListEvent) => void) => {
        listeners.add(listener);
      },
      removeEventListener: (_type: string, listener: (event: MediaQueryListEvent) => void) => {
        listeners.delete(listener);
      },
      addListener: (listener: (event: MediaQueryListEvent) => void) => {
        listeners.add(listener);
      },
      removeListener: (listener: (event: MediaQueryListEvent) => void) => {
        listeners.delete(listener);
      },
      dispatchEvent: (event: Event) => {
        for (const listener of listeners) listener(event as MediaQueryListEvent);
        return true;
      },
    } as MediaQueryList;
  };
}

const SUMMARY: BoardPlanSummary = {
  title: "Compact plan header lane",
  ownerLabel: "Dali",
  ownerPaneId: null,
  plannerLabel: "Planner",
  plannerPaneId: null,
  planningState: "approved",
  executionUnlocked: true,
  done: 99,
  total: 104,
  focus: "Live focus line",
  sectionCount: 3,
  goalVector: "Make the plan header compact on phones so tasks are visible.",
};

async function render(ui: ReactElement): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(ui);
  });
  await act(async () => {
    await Promise.resolve();
  });
  return { host, root };
}

function makeHeader(): ReactElement {
  return (
    <PlanContextCompactHeader
      summary={SUMMARY}
      spec={null}
      onOpenPlan={() => {}}
    />
  );
}

function makeWrappedHeader(): ReactElement {
  return <PlanContextHeader summary={SUMMARY} spec={null} onOpenPlan={() => {}} planId="plan-test" />;
}

function makeWideHeader(plan?: Plan): ReactElement {
  const summary = plan ? deriveBoardPlanSummary(plan, {}) : SUMMARY;
  return (
    <PlanContextWideHeader
      summary={summary}
      spec={null}
      onOpenPlan={() => {}}
      planId={plan?.id ?? "plan-test"}
      plan={plan}
    />
  );
}

const API: ApiConfig = {
  enabled: true,
  host: "127.0.0.1",
  port: 7686,
  token: "test-token",
  cors: false,
};

test("narrow viewport renders compact wrapper even without coarse pointer", async () => {
  window.localStorage.clear();
  setMatchMedia({
    "(pointer: coarse)": false,
    "(hover: none)": false,
    "(max-width: 800px)": false,
    "(max-width: 768px)": true,
  });
  const { host } = await render(makeWrappedHeader());
  assert(host.querySelector('[data-testid="plan-context-compact-bar"]'), "compact bar rendered");
  assert(!host.querySelector('[data-testid="plan-board-plan-goal"]'), "full context gated when collapsed");
});

test("collapsed by default: bar shows state+title+progress, full context NOT mounted", async () => {
  window.localStorage.clear();
  const { host } = await render(makeHeader());
  const bar = host.querySelector('[data-testid="plan-context-compact-bar"]') as HTMLButtonElement;
  assert(bar, "compact bar rendered");
  assert(bar.getAttribute("aria-expanded") === "false", "aria-expanded false when collapsed");
  assert(
    host.querySelector('[data-testid="plan-context-compact-title"]')?.textContent === SUMMARY.title,
    "title in bar",
  );
  assert(
    host.querySelector('[data-testid="plan-context-compact-progress"]')?.textContent === "99/104",
    "progress in bar",
  );
  // Gated RENDER, not CSS-hide: the goal/expanded panel must be absent from the DOM.
  assert(!host.querySelector('[data-testid="plan-context-expanded"]'), "expanded panel not mounted");
  assert(!host.querySelector('[data-testid="plan-board-plan-goal"]'), "goal not mounted when collapsed");
});

test("toggle expands inline, aria-expanded flips, goal + chips render", async () => {
  window.localStorage.clear();
  const { host } = await render(makeHeader());
  const bar = host.querySelector('[data-testid="plan-context-compact-bar"]') as HTMLButtonElement;
  await act(async () => {
    bar.click();
    await Promise.resolve();
  });
  assert(bar.getAttribute("aria-expanded") === "true", "aria-expanded true after expand");
  assert(host.querySelector('[data-testid="plan-context-expanded"]'), "expanded panel mounted");
  assert(host.querySelector('[data-testid="plan-board-plan-goal"]'), "goal rendered when expanded");
  assert(host.querySelector('[data-testid="plan-context-header-chips"]'), "peer chips rendered when expanded");
  await act(async () => {
    bar.click();
    await Promise.resolve();
  });
  assert(bar.getAttribute("aria-expanded") === "false", "aria-expanded false after collapse");
  assert(!host.querySelector('[data-testid="plan-context-expanded"]'), "expanded panel unmounted after collapse");
});

test("chevron has an aria-label", async () => {
  window.localStorage.clear();
  const { host } = await render(makeHeader());
  const chevron = host.querySelector(".plan-context-compact-chevron");
  assert(chevron, "chevron present");
  const label = chevron.getAttribute("aria-label") ?? "";
  assert(/expand|collapse/i.test(label), `chevron aria-label meaningful, got: ${label}`);
});

test("choice persists per width class in UI prefs (localStorage)", async () => {
  window.localStorage.clear();
  // narrow default is collapsed (true)
  assert(readHeaderCollapsed("narrow", true) === true, "narrow default collapsed");
  const { host } = await render(makeHeader());
  const bar = host.querySelector('[data-testid="plan-context-compact-bar"]') as HTMLButtonElement;
  await act(async () => {
    bar.click();
    await Promise.resolve();
  });
  assert(readHeaderCollapsed("narrow", true) === false, "narrow expand persisted");
  assert(readHeaderCollapsed("wide", true) === true, "wide class independent of narrow");
  writeHeaderCollapsed("wide", false);
  assert(readHeaderCollapsed("wide", true) === false, "wide write independent");
  assert(readHeaderCollapsed("narrow", true) === false, "narrow unchanged by wide write");
});

test("wide desktop header defaults expanded with right-side collapse chevron", async () => {
  window.localStorage.clear();
  setMatchMedia({
    "(pointer: coarse)": false,
    "(hover: none)": false,
    "(max-width: 800px)": false,
    "(max-width: 768px)": false,
  });
  const { host } = await render(makeWideHeader());
  assert(host.querySelector('[data-testid="plan-context-wide-chevron"]'), "wide chevron present");
  assert(host.querySelector('[data-testid="plan-board-plan-goal"]'), "full body mounted when expanded");
  const chevron = host.querySelector('[data-testid="plan-context-wide-chevron"]') as HTMLButtonElement;
  await act(async () => {
    chevron.click();
    await Promise.resolve();
  });
  assert(host.querySelector('[data-testid="plan-context-wide-bar"]'), "collapsed bar after toggle");
  assert(!host.querySelector('[data-testid="plan-board-plan-goal"]'), "goal unmounted when collapsed");
});

test("execution plan with drafting default shows Approved chip, not Drafting", async () => {
  window.localStorage.clear();
  const plan: Plan = {
    ...FIXTURE_PLAN,
    mode: "execution",
    planning_state: "drafting",
  };
  const { host } = await render(makeWideHeader(plan));
  const chip = host.querySelector('[data-testid="plan-state-chip"]');
  assert(chip?.textContent === "Approved", `expected Approved, got ${chip?.textContent}`);
  assert(!chip?.className.includes("plan-state-drafting"), "no drafting class");
});

test("remote planner binding renders short id label instead of No planner", async () => {
  window.localStorage.clear();
  const plan: Plan = {
    ...FIXTURE_PLAN,
    planner: { kind: "pane", pane_id: "planner-prod-99", title: null },
  };
  const { host } = await render(makeWideHeader(plan));
  const planner = host.querySelector('[data-testid="plan-peer-chip-planner"]');
  assert(planner?.textContent?.includes("(remote)"), "remote planner label");
  assert(!planner?.textContent?.includes("No planner"), "must not claim no planner");
});

test("parked plan shows Parked state, resume control, and hides heartbeat focus", async () => {
  window.localStorage.clear();
  const plan: Plan = {
    ...FIXTURE_PLAN,
    mode: "paused",
    heartbeat: { current_focus: "task:task-dante still active" },
  };
  const { host } = await render(makeWideHeader(plan));
  assert(host.querySelector('[data-testid="plan-parked-chip"]')?.textContent === "Parked", "parked chip");
  assert(host.querySelector('[data-testid="plan-resume-button"]'), "resume button");
  assert(!host.querySelector('[data-testid="plan-park-button"]'), "park button hidden while parked");
  assert(!host.querySelector('[data-testid="plan-board-plan-focus"]'), "heartbeat focus hidden while parked");
  assert(host.querySelector('[data-testid="plan-board-plan-meta"]')?.textContent?.includes("parked"), "meta says parked");
});

test("park button requires a reason and posts pause request", async () => {
  window.localStorage.clear();
  const calls: Array<{ url: string; init?: RequestInit }> = [];
  Object.defineProperty(globalThis, "fetch", {
    value: async (url: string, init?: RequestInit) => {
      calls.push({ url, init });
      return new Response("{}", { status: 200, headers: { "content-type": "application/json" } });
    },
    configurable: true,
  });
  const originalPrompt = window.prompt;
  window.prompt = () => "waiting for quota";
  const summary = deriveBoardPlanSummary(FIXTURE_PLAN, {});
  const { host } = await render(
    <PlanContextWideHeader
      summary={summary}
      spec={null}
      onOpenPlan={() => {}}
      planId={FIXTURE_PLAN.id}
      plan={FIXTURE_PLAN}
      api={API}
    />,
  );
  const button = host.querySelector('[data-testid="plan-park-button"]') as HTMLButtonElement;
  await act(async () => {
    button.click();
    await Promise.resolve();
  });
  window.prompt = originalPrompt;
  const pauseCalls = calls.filter((call) => call.url.endsWith(`/api/plans/${FIXTURE_PLAN.id}/pause`));
  assert(pauseCalls.length === 1, `expected one pause request, got ${pauseCalls.length}: ${calls.map((c) => c.url).join(", ")}`);
  assert(JSON.parse(String(pauseCalls[0].init?.body)).reason === "waiting for quota", "reason posted");
});

test("resume button posts resume request", async () => {
  window.localStorage.clear();
  const calls: Array<{ url: string; init?: RequestInit }> = [];
  Object.defineProperty(globalThis, "fetch", {
    value: async (url: string, init?: RequestInit) => {
      calls.push({ url, init });
      return new Response("{}", { status: 200, headers: { "content-type": "application/json" } });
    },
    configurable: true,
  });
  const plan: Plan = { ...FIXTURE_PLAN, mode: "paused" };
  const { host } = await render(
    <PlanContextWideHeader
      summary={deriveBoardPlanSummary(plan, {})}
      spec={null}
      onOpenPlan={() => {}}
      planId={plan.id}
      plan={plan}
      api={API}
    />,
  );
  const button = host.querySelector('[data-testid="plan-resume-button"]') as HTMLButtonElement;
  await act(async () => {
    button.click();
    await Promise.resolve();
  });
  const resumeCalls = calls.filter((call) => call.url.endsWith(`/api/plans/${plan.id}/resume`));
  assert(resumeCalls.length === 1, `expected one resume request, got ${resumeCalls.length}: ${calls.map((c) => c.url).join(", ")}`);
});

(async () => {
  let failed = 0;
  for (const [name, fn] of tests) {
    try {
      await fn();
      console.log(`ok ${name}`);
    } catch (e) {
      failed += 1;
      console.error(`FAIL ${name}:`, e);
    }
  }
  if (failed > 0) process.exit(1);
})();
