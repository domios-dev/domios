// Run: npx tsx src/components/Canvas/planView/PlanContextHeader.test.tsx

import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import type { BoardPlanSummary } from "./boardModel";
import { PlanContextCompactHeader, PlanContextHeader } from "./PlanContextHeader";
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
  return <PlanContextHeader summary={SUMMARY} spec={null} onOpenPlan={() => {}} />;
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
