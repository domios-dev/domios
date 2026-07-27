// Run: npx tsx src/components/Canvas/planView/PlanDetailOverlay.test.tsx

import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { FIXTURE_MILESTONE, FIXTURE_PLAN, FIXTURE_SECTION, FIXTURE_TASK } from "../cards/fixtures.testdata";
import { TaskStatus, type Plan, type Task } from "../types";
import { PlanDetailOverlay } from "./PlanDetailOverlay";
import type { ApiConfig } from "../../../lib/types";

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

const API: ApiConfig = {
  enabled: true,
  host: "127.0.0.1",
  port: 7686,
  token: "test-token",
  cors: false,
};

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

test("renders canonical status chip in task detail", async () => {
  const task: Task = { ...FIXTURE_TASK, status: TaskStatus.InReview, review_stage: "qa", qa_state: "queued", status_label: "In QA · queued #1" };
  const plan: Plan = { ...FIXTURE_PLAN, tasks: [task] };
  const { host } = await render(
    <PlanDetailOverlay kind="task" task={task} plan={plan} onClose={() => {}} />,
  );
  const chip = host.querySelector('[data-testid="status-chip"]');
  assert(chip?.textContent?.includes("in review"), "canonical status chip");
  assert(!host.textContent?.includes("In QA · queued #1"), "status label not in header");
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

test("parked plan resume is reachable from plan detail", async () => {
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
    <PlanDetailOverlay kind="plan" plan={plan} api={API} onClose={() => {}} />,
  );
  const resume = host.querySelector('[data-testid="plan-resume-button"]') as HTMLButtonElement;
  assert(resume, "resume is available in plan detail");
  await act(async () => {
    resume.click();
    await Promise.resolve();
  });
  assert(calls.some((call) => call.url.endsWith(`/api/plans/${plan.id}/resume`)), "resume request sent");
});

test("active plan park is reachable from plan detail", async () => {
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
  const { host } = await render(
    <PlanDetailOverlay kind="plan" plan={FIXTURE_PLAN} api={API} onClose={() => {}} />,
  );
  const park = host.querySelector('[data-testid="plan-park-button"]') as HTMLButtonElement;
  assert(park, "park is available in plan detail");
  await act(async () => {
    park.click();
    await Promise.resolve();
  });
  window.prompt = originalPrompt;
  const pause = calls.find((call) => call.url.endsWith(`/api/plans/${FIXTURE_PLAN.id}/pause`));
  assert(pause, "pause request sent");
  assert(JSON.parse(String(pause.init?.body)).reason === "waiting for quota", "park reason sent");
});

test("renders spec detail from specSections", async () => {
  const spec: Plan = { ...FIXTURE_PLAN, tasks: [], sections: [] };
  const { host } = await render(
    <PlanDetailOverlay kind="spec" spec={spec} specSections={[FIXTURE_SECTION]} onClose={() => {}} />,
  );
  assert(host.querySelector('[data-testid="plan-detail-title"]')?.textContent?.includes("spec"), "spec title");
  assert(host.querySelector('[data-testid="canvas-detail-section"][data-section-id="sec-auth"]'), "spec section");
});

test("plan document renders relation chips and opens task chips through overlay API", async () => {
  let openedTaskId: string | null = null;
  let framedSection: string | null = null;
  const specSection = {
    ...FIXTURE_SECTION,
    id: "spec-auth",
    title: "OAuth contract",
    subsections: [],
  };
  const plan: Plan = {
    ...FIXTURE_PLAN,
    spec_links: [{ spec_id: "spec-1", spec_section_id: "spec-auth", plan_section_id: "sec-auth" }],
    tasks: [
      { ...FIXTURE_TASK, id: "task-linked", name: "Linked task", handle: "linked-task", section_ids: ["sec-auth"] },
    ],
  };
  const { host } = await render(
    <PlanDetailOverlay
      kind="plan"
      plan={plan}
      specSections={[specSection]}
      onOpenTask={(taskId) => { openedTaskId = taskId; }}
      onSectionClick={(a) => { framedSection = `${a.variant}:${a.sectionId}`; }}
      onClose={() => {}}
    />,
  );
  const specChip = host.querySelector('[data-testid="canvas-detail-spec-section-chip"]') as HTMLButtonElement;
  assert(specChip?.textContent?.includes("OAuth contract"), "spec section chip uses fetched title");
  await act(async () => {
    specChip.click();
  });
  assert(framedSection === "spec:spec-auth", "spec chip navigates to spec section");
  const taskChip = host.querySelector('[data-testid="canvas-detail-task-chip"]') as HTMLButtonElement;
  assert(taskChip?.textContent === "linked-task", "task chip uses handle");
  await act(async () => {
    taskChip.click();
  });
  assert(openedTaskId === "task-linked", "task chip opens task through overlay API");
});

test("spec coverage header matches plan-state task numbers", async () => {
  const spec: Plan = {
    ...FIXTURE_PLAN,
    sections: [],
    tasks: [
      { ...FIXTURE_TASK, id: "done", status: TaskStatus.Done },
      { ...FIXTURE_TASK, id: "active", status: TaskStatus.InProgress },
      { ...FIXTURE_TASK, id: "cancelled", status: TaskStatus.Cancelled },
    ],
  };
  const { host } = await render(
    <PlanDetailOverlay kind="spec" spec={spec} specSections={[FIXTURE_SECTION]} onClose={() => {}} />,
  );
  assert(host.querySelector('[data-testid="canvas-detail-spec-total"]')?.textContent === "1/2", "cancelled excluded from total");
  assert(host.querySelector('[data-testid="canvas-detail-spec-covered"]')?.textContent?.includes("1 done"), "done count");
  assert(host.querySelector('[data-testid="canvas-detail-spec-partial"]')?.textContent?.includes("1 active"), "active count");
  assert(host.querySelector('[data-testid="canvas-detail-spec-percent"]')?.textContent === "50% done", "task percentage");
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
  assert(btn?.textContent?.includes("Open tab"), "open tab button visible");
  await act(async () => {
    btn.click();
  });
  assert(opened === "pane-worker", "onOpenPane called with Tab id");
});

test("agent identity shows tab title, provider and model from Tab state", async () => {
  const task: Task = { ...FIXTURE_TASK, assigned_pane_id: "pane-worker" };
  const { host } = await render(
    <PlanDetailOverlay
      kind="task"
      task={task}
      panes={{ "pane-worker": { id: "pane-worker", title: "Elgar", agent_type: "claude", agent_session_model: "opus-4.8" } as never }}
      onOpenPane={() => {}}
      onClose={() => {}}
    />,
  );
  const block = host.querySelector('[data-testid="task-agent-identity"]');
  assert(block, "agent identity block visible");
  assert(host.querySelector('[data-testid="task-agent-name"]')?.textContent?.includes("Elgar"), "tab title as agent name");
  assert(host.querySelector('[data-testid="task-agent-provider"]')?.textContent?.includes("claude"), "provider from Tab state");
  assert(host.querySelector('[data-testid="task-agent-model"]')?.textContent?.includes("opus-4.8"), "model from Tab state");
  assert(host.querySelector('[data-testid="task-personality"]')?.textContent === "Elgar", "persona falls back to live tab title");
});

test("personality field renders prominently and overrides the live tab title", async () => {
  const task: Task = { ...FIXTURE_TASK, personality: "Ada", assigned_pane_id: "pane-worker" };
  const { host } = await render(
    <PlanDetailOverlay
      kind="task"
      task={task}
      panes={{ "pane-worker": { id: "pane-worker", title: "Elgar", agent_type: "claude" } as never }}
      onOpenPane={() => {}}
      onClose={() => {}}
    />,
  );
  assert(host.querySelector('[data-testid="task-personality"]')?.textContent === "Ada", "personality field wins over tab title");
  assert(host.querySelector('[data-testid="task-agent-name"]')?.textContent?.includes("Ada"), "agent name uses personality");
});

test("lane block shows tab title with Tab id demoted to secondary", async () => {
  const task: Task = {
    ...FIXTURE_TASK,
    lane: { pane_id: "pane-lane", branch: "lane/x" } as never,
  };
  const { host } = await render(
    <PlanDetailOverlay
      kind="task"
      task={task}
      panes={{ "pane-lane": { id: "pane-lane", title: "ws-scaffold" } as never }}
      onOpenPane={() => {}}
      onClose={() => {}}
    />,
  );
  assert(host.querySelector('[data-testid="task-lane-panel"]'), "lane panel visible");
  assert(host.querySelector('[data-testid="task-lane-pane-title"]')?.textContent === "ws-scaffold", "lane shows live tab title");
  assert(host.querySelector('[data-testid="task-lane-pane-id"]')?.textContent === "pane-lane", "hex Tab id demoted to secondary");
  assert(!host.querySelector('[data-testid="task-lane-pane-dead"]'), "live tab not marked closed");
});

test("lane block marks a closed tab with last known id", async () => {
  const task: Task = {
    ...FIXTURE_TASK,
    lane: { pane_id: "pane-gone", branch: "lane/x" } as never,
  };
  const { host } = await render(
    <PlanDetailOverlay kind="task" task={task} panes={{}} onOpenPane={() => {}} onClose={() => {}} />,
  );
  assert(host.querySelector('[data-testid="task-lane-pane-title"]')?.textContent === "pane-gone", "closed tab falls back to last known id");
  assert(host.querySelector('[data-testid="task-lane-pane-dead"]')?.textContent === "closed", "closed tab marked closed");
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

test("task detail renders milestone activity timeline", async () => {
  const now = Date.now();
  const task: Task = {
    ...FIXTURE_TASK,
    milestones: [{
      ...FIXTURE_MILESTONE,
      id: "ms-detail",
      kind: "decision",
      ts: now - 60_000,
      text: "Chose slide-over layout",
      from: { pane_id: "pane-a", session_id: null, title: "Puccini" },
    }],
  };
  const { host } = await render(
    <PlanDetailOverlay kind="task" task={task} plan={FIXTURE_PLAN} onClose={() => {}} />,
  );
  assert(host.querySelector('[data-testid="plan-task-activity"]'), "activity section");
  assert(host.querySelector('[data-testid="plan-task-activity-entry"]')?.getAttribute("data-milestone-id") === "ms-detail", "task milestone");
  assert(host.querySelector('[data-testid="plan-task-activity-age"]')?.textContent === "1m", "relative age in detail");
  assert(host.querySelector('[data-testid="plan-task-activity-timestamp"]')?.textContent, "absolute timestamp in detail");
});

test("task detail renders Description above Activity", async () => {
  const task: Task = {
    ...FIXTURE_TASK,
    body: "Reorder the sections",
    milestones: [{
      ...FIXTURE_MILESTONE,
      id: "ms-order",
      kind: "status_update",
      ts: Date.now() - 60_000,
      text: "Working point",
      from: { pane_id: "pane-a", session_id: null, title: "Puccini" },
    }],
  };
  const { host } = await render(
    <PlanDetailOverlay kind="task" task={task} plan={FIXTURE_PLAN} onClose={() => {}} />,
  );
  const description = host.querySelector('[data-testid="canvas-detail-description"]');
  const activity = host.querySelector('[data-testid="plan-task-activity"]');
  assert(description && activity, "both description and activity present");
  const FOLLOWING = 4;
  assert(
    (description!.compareDocumentPosition(activity!) & FOLLOWING) !== 0,
    "description renders before activity in DOM order",
  );
});

test("plan document renders recent activity rail with click-through", async () => {
  let openedTaskId: string | null = null;
  const task: Task = {
    ...FIXTURE_TASK,
    id: "task-active",
    handle: "activity-task",
    milestones: [{
      ...FIXTURE_MILESTONE,
      id: "ms-plan",
      kind: "status_update",
      ts: 1_700_000_000_000,
      text: "Worker reported in_review",
      from: { pane_id: "pane-a", session_id: null, title: "Puccini" },
    }],
  };
  const plan: Plan = { ...FIXTURE_PLAN, tasks: [task] };
  const { host } = await render(
    <PlanDetailOverlay
      kind="plan"
      plan={plan}
      onOpenTask={(taskId) => { openedTaskId = taskId; }}
      onClose={() => {}}
    />,
  );
  const item = host.querySelector('[data-testid="plan-recent-activity-item"]') as HTMLButtonElement;
  assert(item?.getAttribute("data-task-id") === "task-active", "recent activity task id");
  await act(async () => {
    item.click();
  });
  assert(openedTaskId === "task-active", "recent activity opens task");
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
