// Run: npx tsx src/components/Canvas/planView/PlanContextHeader.test.tsx

import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import type { BoardPlanSummary } from "./boardModel";
import { deriveBoardPlanSummary } from "./boardModel";
import { FIXTURE_PLAN } from "../cards/fixtures.testdata";
import type { Plan } from "../types";
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
  ownerTabId: null,
  plannerLabel: "Planner",
  plannerTabId: null,
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

test("header chevron precedes plan label in compact, wide-collapsed, and wide-expanded variants", async () => {
  window.localStorage.clear();
  const compact = await render(makeHeader());
  const compactChevron = compact.host.querySelector(".plan-context-compact-chevron");
  const compactTitle = compact.host.querySelector('[data-testid="plan-context-compact-title"]');
  assert(compactChevron && compactTitle, "compact chevron and title render");
  assert((compactChevron.compareDocumentPosition(compactTitle) & 4) !== 0, "compact chevron precedes title");

  const wide = await render(makeWideHeader());
  const wideChevron = wide.host.querySelector('[data-testid="plan-context-wide-chevron"]');
  const wideTitle = wide.host.querySelector('[data-testid="plan-board-plan-title"]');
  assert(wideChevron && wideTitle, "wide-expanded chevron and title render");
  assert((wideChevron.compareDocumentPosition(wideTitle) & 4) !== 0, "wide-expanded chevron precedes title");
  await act(async () => {
    (wideChevron as HTMLButtonElement).click();
    await Promise.resolve();
  });
  const collapsedChevron = wide.host.querySelector(".plan-context-compact-chevron");
  const collapsedTitle = wide.host.querySelector('[data-testid="plan-context-compact-title"]');
  assert(collapsedChevron && collapsedTitle, "wide-collapsed chevron and title render");
  assert((collapsedChevron.compareDocumentPosition(collapsedTitle) & 4) !== 0, "wide-collapsed chevron precedes title");
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

test("wide desktop header defaults expanded with leading collapse chevron", async () => {
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
    planner: { kind: "tab", tab_id: "planner-prod-99", title: null },
  };
  const { host } = await render(makeWideHeader(plan));
  const planner = host.querySelector('[data-testid="plan-peer-chip-planner"]');
  assert(planner?.textContent?.includes("(remote)"), "remote planner label");
  assert(!planner?.textContent?.includes("No planner"), "must not claim no planner");
});

test("park button is absent from header while parked state remains passive", async () => {
  window.localStorage.clear();
  const plan: Plan = {
    ...FIXTURE_PLAN,
    mode: "paused",
    heartbeat: { current_focus: "task:task-dante still active" },
  };
  const { host } = await render(makeWideHeader(plan));
  assert(host.querySelector('[data-testid="plan-parked-chip"]')?.textContent === "Parked", "parked chip");
  assert(!host.querySelector('[data-testid="plan-resume-button"]'), "resume button absent from header");
  assert(!host.querySelector('[data-testid="plan-park-button"]'), "park button hidden while parked");
  assert(!host.querySelector('[data-testid="plan-board-plan-focus"]'), "heartbeat focus hidden while parked");
  assert(host.querySelector('[data-testid="plan-board-plan-meta"]')?.textContent?.includes("parked"), "meta says parked");
});

test("no red skills state is reachable in the header for empty or rejected skills fetches", async () => {
  window.localStorage.clear();
  let fetchCalls = 0;
  Object.defineProperty(globalThis, "fetch", {
    value: async () => {
      fetchCalls += 1;
      throw new Error("skills unavailable");
    },
    configurable: true,
  });
  const { host } = await render(makeWideHeader(FIXTURE_PLAN));
  await act(async () => {
    await Promise.resolve();
  });
  assert(fetchCalls === 0, "header does not fetch skills");
  assert(!host.querySelector('[data-testid="plan-skills-section"]'), "empty skills section absent from header");
  assert(!host.querySelector('[data-testid="plan-skills-error"]'), "rejected skills error absent from header");
});

test("body priority is status, goal, then compact plan/spec actions", async () => {
  window.localStorage.clear();
  const spec = {
    specId: "spec-release-stability",
    title: "Release stability spec with a long title",
    sectionCount: 4,
    leadSection: "Header polish",
  };
  const summary = { ...deriveBoardPlanSummary(FIXTURE_PLAN, {}), goalVector: "Keep plan context prioritized." };
  const { host } = await render(
    <PlanContextWideHeader
      summary={summary}
      spec={spec}
      onOpenPlan={() => {}}
      onOpenSpec={() => {}}
      planId={FIXTURE_PLAN.id}
      plan={FIXTURE_PLAN}
    />,
  );
  const status = host.querySelector('[data-testid="plan-context-header-chips"]');
  const goal = host.querySelector('[data-testid="plan-board-plan-goal"]');
  const main = host.querySelector('[data-testid="plan-context-header-main"]');
  assert(status && goal && main, "status, goal, and actions render");
  assert((status.compareDocumentPosition(goal) & 4) !== 0, "status precedes goal");
  assert((goal.compareDocumentPosition(main) & 4) !== 0, "goal precedes actions");
  assert(main.querySelector('[data-testid="plan-board-plan-title"]'), "plan title in main row");
  assert(main.querySelector('[data-testid="plan-board-plan-meta"]'), "plan meta in main row");
  assert(main.querySelector('[data-testid="plan-board-spec-title"]'), "spec trigger in main row");
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
