// Run: npx tsx src/components/Terminal/WorkerTaskStrip.test.tsx

import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { WorkerTaskStrip } from "./WorkerTaskStrip";
import { presenceColor, type PresenceAgent } from "../Canvas/presence/identity";
import type { Task } from "../Canvas/types";

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
  const window = new Window({ url: "http://localhost/worker-task-strip" });
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

const emptyCanvas = { x: null, y: null, collapsed: false };
const TASK: Task = {
  id: "task-r3",
  name: "R3 — Research World/Figma canvas architecture",
  type: "research",
  model: null,
  model_rationale: null,
  status: "in_progress",
  status_label: null,
  handle: "r3-research-world",
  section_ids: [],
  body: "",
  intent: null,
  parent_task_id: null,
  assigned_pane_id: "f61e6203",
  peer: null,
  canvas: emptyCanvas,
  milestones: [],
};
const AGENT: PresenceAgent = {
  id: "f61e6203",
  label: "backend-dev",
  role: "backend-dev",
  color: presenceColor("f61e6203"),
};

async function run() {
  setupDom();

  // Absent task → renders nothing (honest, no chrome).
  {
    const { host, root } = await render(<WorkerTaskStrip task={null} agent={AGENT} />);
    ok("no task → renders nothing", host.querySelector('[data-testid="worker-task-strip"]') === null);
    await act(async () => root.unmount());
  }

  // Present task → handle + name + status + branch + agent dot, and click fires.
  {
    let clicks = 0;
    const { host, root } = await render(
      <WorkerTaskStrip task={TASK} branch="feat/canvas" agent={AGENT} onOpen={() => { clicks++; }} />,
    );
    const strip = host.querySelector('[data-testid="worker-task-strip"]') as HTMLElement | null;
    ok("strip renders", !!strip);
    ok("strip carries task id", strip?.getAttribute("data-task-id") === "task-r3");
    ok("name shown", host.querySelector('[data-testid="worker-task-strip-name"]')?.textContent === TASK.name);
    ok("handle shown", host.querySelector('[data-testid="worker-task-strip-handle"]')?.textContent?.includes("r3-research-world") === true);
    ok("status shown", host.querySelector('[data-testid="worker-task-strip-status"]')?.textContent === "in progress");
    ok("branch shown", host.querySelector('[data-testid="worker-task-strip-branch"]')?.textContent === "feat/canvas");
    ok("agent dot shown", host.querySelector('[data-testid="canvas-agent-circle"]')?.getAttribute("data-agent-id") === "f61e6203");
    await act(async () => {
      (strip as unknown as HTMLButtonElement)?.click();
    });
    ok("click fires onOpen", clicks === 1);
    await act(async () => root.unmount());
  }

  // No branch → branch chip absent (no empty chrome).
  {
    const { host, root } = await render(<WorkerTaskStrip task={TASK} agent={AGENT} />);
    ok("no branch → no branch chip", host.querySelector('[data-testid="worker-task-strip-branch"]') === null);
    await act(async () => root.unmount());
  }

  console.log(`\n${passed} passed, ${failed} failed`);
  if (failed > 0) process.exit(1);
}

void run();
