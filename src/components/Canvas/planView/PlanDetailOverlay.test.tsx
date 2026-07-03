// Run: npx tsx src/components/Canvas/planView/PlanDetailOverlay.test.tsx

import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { FIXTURE_PLAN, FIXTURE_SECTION, FIXTURE_TASK } from "../cards/fixtures.testdata";
import { TaskStatus, type Plan, type Task } from "../types";
import { PlanDetailOverlay } from "./PlanDetailOverlay";

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
  const window = new Window({ url: "http://localhost/plan-detail" });
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
  Object.defineProperty(globalThis, "KeyboardEvent", { value: window.KeyboardEvent, configurable: true });
}

setupDom();

async function render(ui: ReactElement): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  host.style.width = "1200px";
  host.style.height = "800px";
  host.style.position = "relative";
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

test("renders task detail body", async () => {
  const { host } = await render(
    <PlanDetailOverlay kind="task" task={FIXTURE_TASK} onClose={() => {}} />,
  );
  assert(host.querySelector('[data-testid="plan-detail-overlay"]'), "overlay visible");
  assert(host.querySelector('[data-testid="plan-detail-overlay"]')?.getAttribute("data-detail-mode") === "slide-over", "task opens as slide-over");
  assert(host.querySelector('[data-testid="plan-detail-title"]')?.textContent === "Dante", "task title");
  assert(host.querySelector('[data-testid="plan-detail-kind"]')?.textContent === "task", "task kind");
  assert(host.querySelector('[data-testid="canvas-detail-content"]'), "task body");
});

test("renders in-QA chip in task detail", async () => {
  const task: Task = { ...FIXTURE_TASK, status: TaskStatus.InReview, review_stage: "qa", qa_state: "queued" };
  const plan: Plan = { ...FIXTURE_PLAN, tasks: [task] };
  const { host } = await render(
    <PlanDetailOverlay kind="task" task={task} plan={plan} onClose={() => {}} />,
  );
  assert(host.textContent?.includes("In QA · queued #1"), "qa detail chip");
});

test("renders plan detail with progress", async () => {
  const plan: Plan = {
    ...FIXTURE_PLAN,
    goal: { vector: "Ship auth" },
    tasks: [
      { ...FIXTURE_TASK, status: TaskStatus.Done },
      { ...FIXTURE_TASK, id: "task-2", status: TaskStatus.InProgress },
    ],
  };
  const { host } = await render(
    <PlanDetailOverlay kind="plan" plan={plan} onClose={() => {}} />,
  );
  assert(host.querySelector('[data-testid="plan-detail-overlay"]')?.getAttribute("data-detail-mode") === "document", "plan opens as document overlay");
  assert(host.querySelector('[data-testid="plan-detail-title"]')?.textContent === "Auth rollout", "plan title");
  assert(host.querySelector('[data-testid="canvas-detail-progress"]'), "plan progress");
  assert(host.querySelector('[data-testid="canvas-detail-section"]'), "plan section");
});

test("renders spec detail from specSections", async () => {
  const spec: Plan = { ...FIXTURE_PLAN, tasks: [], sections: [] };
  const { host } = await render(
    <PlanDetailOverlay kind="spec" spec={spec} specSections={[FIXTURE_SECTION]} onClose={() => {}} />,
  );
  assert(host.querySelector('[data-testid="plan-detail-title"]')?.textContent?.includes("spec"), "spec title");
  assert(host.querySelector('[data-testid="canvas-detail-section"][data-section-id="sec-auth"]'), "spec section");
});

test("Esc closes overlay", async () => {
  let closed = false;
  const { host } = await render(
    <PlanDetailOverlay kind="task" task={FIXTURE_TASK} onClose={() => { closed = true; }} />,
  );
  const overlay = host.querySelector('[data-testid="plan-detail-overlay"]') as HTMLElement;
  await act(async () => {
    overlay.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
  });
  assert(closed, "onClose called on Escape");
});

test("focus trap wraps Tab within panel", async () => {
  const plan: Plan = { ...FIXTURE_PLAN, goal: { vector: "Ship auth" } };
  const { host } = await render(
    <PlanDetailOverlay kind="plan" plan={plan} onClose={() => {}} />,
  );
  const closeBtn = host.querySelector('[data-testid="plan-detail-close"]') as HTMLButtonElement;
  const navButtons = Array.from(host.querySelectorAll('[data-testid="canvas-detail-section-nav"]')) as HTMLButtonElement[];
  const lastNav = navButtons[navButtons.length - 1];
  await act(async () => {
    lastNav.focus();
    lastNav.dispatchEvent(new KeyboardEvent("keydown", { key: "Tab", bubbles: true, cancelable: true }));
  });
  assert(document.activeElement === closeBtn, "tab wraps from last focusable to first");
});

test("open-pane action invokes callback", async () => {
  let opened: string | null = null;
  const task: Task = { ...FIXTURE_TASK, assigned_pane_id: "pane-worker" };
  const { host } = await render(
    <PlanDetailOverlay
      kind="task"
      task={task}
      panes={{ "pane-worker": { id: "pane-worker", title: "Puccini" } as never }}
      onOpenPane={(id) => { opened = id; }}
      onClose={() => {}}
    />,
  );
  const btn = host.querySelector('[data-testid="plan-detail-open-pane"]') as HTMLButtonElement;
  assert(btn?.textContent?.includes("Show agent"), "show agent button visible");
  await act(async () => {
    btn.click();
  });
  assert(opened === "pane-worker", "onOpenPane called with pane id");
});

test("task detail renders linked plan sections as navigable links", async () => {
  let sectionId: string | null = null;
  let planId: string | null = null;
  const { host } = await render(
    <PlanDetailOverlay
      kind="task"
      task={FIXTURE_TASK}
      plan={FIXTURE_PLAN}
      planId={FIXTURE_PLAN.id}
      onSectionClick={(a) => {
        sectionId = a.sectionId;
        planId = a.planId;
      }}
      onClose={() => {}}
    />,
  );
  const link = host.querySelector('[data-testid="plan-detail-task-section-link"]') as HTMLButtonElement;
  assert(link?.textContent === "Authentication", "linked section title");
  await act(async () => {
    link.click();
  });
  assert(sectionId === "sec-auth", "section click id");
  assert(planId === "plan-alpha", "section click plan id");
});

test("task detail exposes attachment thumbnail slot", async () => {
  const { host } = await render(
    <PlanDetailOverlay
      kind="task"
      task={FIXTURE_TASK}
      renderTaskAttachmentThumbnails={() => <button type="button">Preview</button>}
      onClose={() => {}}
    />,
  );
  const slot = host.querySelector('[data-testid="plan-detail-attachment-thumbnails"]') as HTMLElement;
  assert(slot?.textContent === "Preview", "attachment slot content");
});

test("detail body supports text selection", async () => {
  const { host } = await render(
    <PlanDetailOverlay kind="task" task={FIXTURE_TASK} onClose={() => {}} />,
  );
  const body = host.querySelector('[data-testid="plan-detail-body"]') as HTMLElement;
  assert(body.className.includes("plan-detail-selectable"), "detail body selectable");
  const title = host.querySelector('[data-testid="plan-detail-title"]') as HTMLElement;
  assert(title.className.includes("plan-task-selectable"), "detail title selectable");
});

test("focus return remains on opener after selected task changes", async () => {
  const opener = document.createElement("button");
  opener.textContent = "Open";
  document.body.appendChild(opener);
  opener.focus();
  const host = document.createElement("div");
  host.style.width = "1200px";
  host.style.height = "800px";
  host.style.position = "relative";
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(<PlanDetailOverlay kind="task" task={FIXTURE_TASK} onClose={() => {}} />);
  });
  const secondTask: Task = { ...FIXTURE_TASK, id: "task-2", name: "Virgil" };
  await act(async () => {
    root.render(<PlanDetailOverlay kind="task" task={secondTask} onClose={() => {}} />);
  });
  await act(async () => {
    root.unmount();
  });
  assert(document.activeElement === opener, "focus returns to initial opener");
  opener.remove();
  host.remove();
});

let passed = 0;
(async () => {
  for (const [name, fn] of tests) {
    try {
      await fn();
      passed++;
      console.log(`  ✓ ${name}`);
    } catch (e) {
      console.error(`  ✗ ${name}`);
      console.error(e);
      process.exit(1);
    }
  }
  console.log(`\n✓ ${passed}/${tests.length} PlanDetailOverlay tests passed`);
})().catch((e) => {
  console.error(e);
  process.exit(1);
});
