// Run: npx tsx src/components/Terminal/TabSessionChat.planref.test.tsx

import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { LinkedText } from "../../../packages/chat-ui/src/codeterm/LinkedText";
import { CodetermActionCard } from "../../../packages/chat-ui/src/toolcard/CodetermActionCard";
import type { Workspace } from "../../lib/types";
import { selectTabCommunicationNameMap } from "../../store/multiplexer/selectors";
import type { ExtendedState } from "../../store/multiplexer/tree";
import type { PlanIndexEntry, ResolvedPlanRef } from "../Canvas/plansApi";
import { chatPlanRefView } from "./chatPlanRef";
import { useChatTabNameResolver } from "./useChatTabNameResolver";

function SendRow({ names, refId, onFocusTab }: { names: Map<string, string>; refId: string; onFocusTab: (ref: string) => void }) {
  const resolveTabName = useChatTabNameResolver(names);
  const command = `domios send --tab ${refId} "hello"`;
  return <CodetermActionCard
    event={{ id: "send", phase: "result", status: "success", kind: "codeterm", summary: command, detail: command, message: { id: "send", type: "tool_call", content: "" } }}
    expanded={false}
    onToggle={() => {}}
    resolveTabName={resolveTabName}
    onFocusTab={onFocusTab}
  />;
}

async function verifySendIdentity() {
  const liveId = "deadbeef1111";
  const closedId = "deadbeef2222";
  const state: ExtendedState = {
    workspaces: [{ id: "ws", name: "Workspace", tabs: [liveId, closedId], active_tab_id: liveId, cwd: null }],
    tabs: Object.fromEntries([[liveId, "Courier"], [closedId, "Archivist"]].map(([id, title]) => [id, {
      id, title, pty_id: id, tab_type: "Terminal", cwd: null, agent_type: null, agent_state: null,
    }])),
    teams: [], active_workspace_id: "ws", focused_tab_id: liveId, notifications: [], tabStatuses: {},
    toasts: [], activeTabIds: {}, generation: 1, pendingCloses: [],
  };
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  const focused: string[] = [];
  async function show(refId: string) {
    await act(async () => root.render(<SendRow names={selectTabCommunicationNameMap(state)} refId={refId} onFocusTab={(ref) => focused.push(ref)} />));
    return host.querySelector<HTMLButtonElement>('[data-testid="codeterm-focus-tab"]');
  }
  assert(await show(closedId), "live full recipient has an opener before closure");
  state.workspaces[0].tabs = [liveId];
  assert(!await show(closedId), "closed full recipient has no opener after its prefix becomes a live alias");
  assert(host.querySelector('[data-testid="chat-sent-header"]')?.textContent?.includes(closedId), "closed full recipient retains its complete ID");
  assert(!host.textContent?.includes("Courier") && !host.textContent?.includes("Archivist"), "closed recipient borrows neither a live prefix title nor a stale title");
  assert(!await show("deadbeef3333"), "missing full recipient has no opener despite a matching live short alias");
  assert(host.querySelector('[data-testid="chat-sent-header"]')?.textContent?.includes("deadbeef3333"), "missing recipient retains its complete ID");
  for (const ref of [liveId, "deadbeef"]) {
    const opener = await show(ref);
    assert(opener && host.textContent?.includes("Courier"), "exact live ID and explicitly supplied unique short ref resolve");
    await act(async () => opener.click());
    assert(focused[focused.length - 1] === ref, "resolved opener forwards the supplied authoritative ref");
  }
  await act(async () => root.unmount());
  host.remove();
  console.log("✓ Chat caller resolves exact live refs and never aliases missing or closed full IDs");
}

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
  await verifySendIdentity();
  const workspace: Workspace = {
    id: "ws-codeterm",
    name: "CodeTerm",
    cwd: "D:\\Developer\\codeterm",
    active_tab_id: null,
    tabs: [],
  };
  const entry: PlanIndexEntry = {
    plan_id: "pl_xcxs29",
    file_path: "C:\\Users\\dev\\.domios\\plans.db",
    canonical_project_path: "\\\\?\\D:\\Developer\\codeterm",
    owner: { tab_id: "tab-owner", session_id: null, title: "Owner" },
    title: "Canonical ownership",
    status: "active",
    updated_at: 1,
    assigned_tab_ids: [],
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
    owner_tab_id: "tab-owner",
  };
  const view = chatPlanRefView(resolved, [entry], [workspace], false);
  const taskView = chatPlanRefView({
    ...resolved,
    target_kind: "task",
    target_id: "task_legacy",
    task_title: "Legacy task",
  }, [entry], [workspace], false);
  assert(taskView.taskKey === "LEGACY", "resolved task ref exposes the existing public display-key fallback");
  const missingTask = chatPlanRefView({ ...resolved, target_kind: "task", target_id: "task_gone", state: "missing" }, [entry], [workspace], false);
  assert(missingTask.title === "task_gone", "a missing task is labelled by its own id, never by its plan");
  let opened = false;
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(
      <LinkedText
        text={'Open <domios type="plan" id="pl_xcxs29" />'}
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
