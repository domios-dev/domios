// Run: npx tsx src/components/Terminal/PaneSessionChat.planref.test.tsx

import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { LinkedText } from "../../../packages/chat-ui/src/codeterm/LinkedText";
import type { Workspace } from "../../lib/types";
import type { PlanIndexEntry, ResolvedPlanRef } from "../Canvas/plansApi";
import { chatPlanRefView } from "./chatPlanRef";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/chat-plan-ref" });
  for (const key of ["window", "document", "navigator", "HTMLElement", "Element", "Node", "Event"] as const) {
    Object.defineProperty(globalThis, key, { value: (window as never)[key], configurable: true });
  }
}

async function main() {
  setupDom();
  const workspace: Workspace = {
    id: "ws-codeterm",
    name: "CodeTerm",
    cwd: "D:\\Developer\\codeterm",
    active_tab_id: null,
    tabs: [],
  };
  const entry: PlanIndexEntry = {
    plan_id: "pl_xcxs29",
    file_path: "C:\\Users\\User\\.codeterm\\plans.db",
    canonical_project_path: "\\\\?\\D:\\Developer\\codeterm",
    owner: { pane_id: "pane-owner", session_id: null, title: "Owner" },
    title: "Canonical ownership",
    status: "active",
    updated_at: 1,
    assigned_pane_ids: [],
    task_count: 1,
  };
  const resolved: ResolvedPlanRef = {
    plan_id: entry.plan_id,
    target_kind: "plan",
    state: "resolved",
    title: entry.title,
    plan_title: entry.title,
    file_path: entry.file_path,
    canonical_project_path: entry.canonical_project_path,
    status: "active",
    owner_pane_id: "pane-owner",
  };
  const view = chatPlanRefView(resolved, [entry], [workspace], false);
  let opened = false;
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(
      <LinkedText
        text="Open pl_xcxs29"
        resolvePlanRef={() => view}
        onOpenPlan={() => { opened = true; }}
      />,
    );
  });

  const link = host.querySelector<HTMLButtonElement>('[data-testid="chat-plan-link"]');
  assert(link != null, "live DB-backed plan renders a plan link");
  assert(link.dataset.refState === "resolved", "canonical ownership preserves the resolved state");
  assert(!link.disabled, "resolved plan link is clickable");
  assert(!host.textContent?.includes("Workspace unavailable"), "no unavailable suffix is rendered");
  await act(async () => link.click());
  assert(opened, "click reaches the Chat deep-link handler");

  await act(async () => root.unmount());
  host.remove();
  console.log("✓ live workspace-owned DB-backed Chat ref is resolved and clickable");
}

void main();
