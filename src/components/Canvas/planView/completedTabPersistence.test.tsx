import { act, type ReactElement } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { FIXTURE_PLAN, FIXTURE_TASK } from "../cards/fixtures.testdata";
import { PlanSidePanelLayer } from "../PlanSidePanelHost";
import { planSidePanelStore } from "../planSidePanelStore";
import { TaskStatus, type Plan } from "../types";
import { PlanBoardView } from "./PlanBoardView";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/plan-board" });
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
}

async function render(ui: ReactElement) {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () => root.render(ui));
  return { host, root };
}

async function main() {
  setupDom();
  const plan: Plan = {
    ...FIXTURE_PLAN,
    tasks: [{
      ...FIXTURE_TASK,
      id: "completed-from-closed-tab",
      name: "Completed after tab closure",
      status: TaskStatus.Done,
      assigned_tab_id: "closed-worker-tab",
    }],
  };
  const open = (target: { planId: string; taskId?: string }) => planSidePanelStore.open(target);
  const { host, root } = await render(
    <>
      <PlanBoardView mode="focus" planId={plan.id} plans={[plan]} onOpenPlanTarget={open} />
      <PlanSidePanelLayer plans={[plan]} onOpenPlanTarget={open} />
    </>,
  );
  const done = host.querySelector('[data-testid="plan-board-column"][data-column="done"]');
  assert(done, "a completed task remains available in the persistent Done view after its tab is absent");
  const card = done.querySelector('[data-task-id="completed-from-closed-tab"]') as HTMLElement | null;
  assert(card, "the Done view keeps the completed task record readable");
  await act(async () => card.click());
  assert(
    host.querySelector('[data-testid="plan-side-panel-title"]')?.textContent === "Completed after tab closure",
    "the retained Done task opens its durable detail",
  );
  await act(async () => root.unmount());
  console.log("completed tab persistence regression passed");
}

void main();
