// Run: npx tsx src/components/Terminal/ComposerPlanActivityStrip.test.tsx

import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { ComposerPlanActivityStrip } from "./ComposerPlanActivityStrip";
import type { PaneComposerActivity } from "../Canvas/paneComposerActivity";

let passed = 0;
let failed = 0;
function ok(label: string, cond: boolean) {
  if (cond) {
    console.log(`  ✓ ${label}`);
    passed++;
  } else {
    console.error(`  ✗ ${label}`);
    failed++;
  }
}

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/composer-plan-activity-strip" });
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
}

async function render(ui: ReactElement): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(ui);
  });
  return { host, root };
}

const ORCH: PaneComposerActivity = {
  kind: "orchestrating",
  planId: "pl_hidden",
  planTitle: "Canvas v2 — the full build (dogfood)",
  extraPlans: 1,
};

const WORK: PaneComposerActivity = {
  kind: "working",
  planId: "pl_hidden",
  planTitle: "Canvas v2 — the full build (dogfood)",
  taskId: "task_hidden",
  taskTitle: "Composer plan activity context above input",
  taskStatus: "in_progress",
};

const PLAN: PaneComposerActivity = {
  kind: "planning",
  planId: "pl_hidden",
  planTitle: "Canvas v2 — the full build (dogfood)",
};

async function run() {
  setupDom();

  {
    const { host, root } = await render(<ComposerPlanActivityStrip activity={ORCH} />);
    const strip = host.querySelector('[data-testid="composer-plan-activity-strip"]');
    ok("orchestrator strip renders", !!strip);
    ok("chip baseline class", strip?.classList.contains("ct-composer-activity-chip") === true);
    ok("kind attribute", strip?.getAttribute("data-activity-kind") === "orchestrating");
    ok("prefix", host.querySelector('[data-testid="composer-plan-activity-prefix"]')?.textContent === "Orchestrating:");
    ok("primary title", host.querySelector('[data-testid="composer-plan-activity-primary"]')?.textContent?.includes("Canvas v2") === true);
    ok("no raw plan id in visible text", !strip?.textContent?.includes("pl_hidden"));
    ok("extra plans chip", host.querySelector('[data-testid="composer-plan-activity-extra-plans"]')?.textContent === "+1");
    ok("no secondary for orchestrator", host.querySelector('[data-testid="composer-plan-activity-secondary"]') === null);
    await act(async () => root.unmount());
  }

  {
    let clicks = 0;
    const { host, root } = await render(
      <ComposerPlanActivityStrip activity={WORK} taskStatus="in_progress" onOpen={() => { clicks += 1; }} />,
    );
    const strip = host.querySelector('[data-testid="composer-plan-activity-strip"]') as HTMLButtonElement | null;
    ok("worker strip renders", !!strip);
    ok("kind working", strip?.getAttribute("data-activity-kind") === "working");
    ok("prefix working", host.querySelector('[data-testid="composer-plan-activity-prefix"]')?.textContent === "Working on:");
    ok("task title shown", host.querySelector('[data-testid="composer-plan-activity-primary"]')?.textContent === "Composer plan activity context above input");
    ok("plan title secondary", host.querySelector('[data-testid="composer-plan-activity-secondary"]')?.textContent?.includes("Canvas v2") === true);
    ok("status pill", host.querySelector('[data-testid="composer-current-task"]')?.textContent === "in progress");
    ok("no raw ids in visible text", !strip?.textContent?.includes("task_hidden") && !strip?.textContent?.includes("pl_hidden"));
    await act(async () => { strip?.click(); });
    ok("click fires onOpen", clicks === 1);
    await act(async () => root.unmount());
  }

  {
    let clicks = 0;
    const { host, root } = await render(
      <ComposerPlanActivityStrip activity={PLAN} onOpen={() => { clicks += 1; }} />,
    );
    const strip = host.querySelector('[data-testid="composer-plan-activity-strip"]') as HTMLButtonElement | null;
    ok("planner strip renders", !!strip);
    ok("kind planning", strip?.getAttribute("data-activity-kind") === "planning");
    ok("prefix planning", host.querySelector('[data-testid="composer-plan-activity-prefix"]')?.textContent === "Planning:");
    ok("plan title shown", host.querySelector('[data-testid="composer-plan-activity-primary"]')?.textContent?.includes("Canvas v2") === true);
    ok("no task status chip for planning", host.querySelector('[data-testid="composer-current-task"]') === null);
    ok("no secondary for planning", host.querySelector('[data-testid="composer-plan-activity-secondary"]') === null);
    ok("no raw plan id in visible text", !strip?.textContent?.includes("pl_hidden"));
    await act(async () => { strip?.click(); });
    ok("planning click fires onOpen (opens plan)", clicks === 1);
    await act(async () => root.unmount());
  }

  console.log(`\n${passed} passed, ${failed} failed`);
  if (failed > 0) process.exit(1);
}

void run();
