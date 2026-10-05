// Run: node scripts/run-ts-unit-tests.mjs src/components/Sidebar/WorkspaceNavigationActions.test.tsx

import assert from "node:assert/strict";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { WorkspaceNavigationActions } from "./WorkspaceNavigationActions";

async function main() {
  const win = new Window({ url: "http://localhost" });
  for (const key of ["window", "document", "navigator", "HTMLElement", "Element", "Node", "Event", "MouseEvent", "KeyboardEvent"]) {
    Object.defineProperty(globalThis, key, { configurable: true, value: (win as unknown as Record<string, unknown>)[key] });
  }
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const opened: string[] = [];
  try {
    await act(async () => root.render(<WorkspaceNavigationActions
      workspaceId="workspace-1"
      workspaceName="Project"
      onOpenMap={(id) => opened.push(`map:${id}`)}
      onOpenJobs={(id) => opened.push(`jobs:${id}`)}
      activeJobsCount={3}
    />));
    const map = host.querySelector<HTMLButtonElement>('button[aria-label="Map"]');
    const jobs = host.querySelector<HTMLButtonElement>('button[aria-label="Jobs, 3 active"]');
    assert.ok(map && jobs, "the Map and Jobs buttons are present with announced active count");
    assert.equal(map.type, "button");
    assert.equal(jobs.type, "button");
    assert.equal(map.tabIndex, 0);
    assert.equal(jobs.tabIndex, 0);
    assert.ok(!map.className.includes("opacity-0") && !jobs.className.includes("opacity-0"), "buttons are not hover gated");
    await act(async () => map.focus());
    assert.equal(document.activeElement, map);
    await act(async () => jobs.focus());
    assert.equal(document.activeElement, jobs);
    await act(async () => { map.click(); jobs.click(); });
    assert.deepEqual(opened, ["map:workspace-1", "jobs:workspace-1"]);
    console.log("Workspace Map and Jobs actions are visible, named, keyboard-focusable, and carry the active count");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
}

void main().catch((error: unknown) => { console.error(error); process.exitCode = 1; });
