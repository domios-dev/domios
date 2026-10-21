import assert from "node:assert/strict";
import { test } from "node:test";
import { Window } from "happy-dom";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { __setInvokeForTest } from "../../lib/bridge";
import { ChecklistProposalCard } from "./ChecklistProposalCard";
import { boundedChecklist, type AgentChecklist } from "./agentChecklistModel";

const dom = new Window({ url: "http://localhost" });
Object.defineProperty(globalThis, "window", { value: dom, configurable: true });
Object.defineProperty(globalThis, "document", { value: dom.document, configurable: true });
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
const checklist: AgentChecklist = { tabId: "worker", generation: 1, revision: 3, proposal: { state: "proposed" }, items: [
  { id: "criterion", title: "Deliver result", locked: true, state: "in_progress" },
  { id: "own", title: "Inspect", state: "open" },
] };
function button(host: HTMLElement, text: string) {
  const found = [...host.querySelectorAll("button")].find((button) => button.textContent === text);
  assert.ok(found, `missing ${text}`); return found;
}

test("proposal state survives the bounded UI projection", () => {
  for (const state of ["proposed", "approved", "amended"] as const) {
    assert.equal(boundedChecklist({ tab_id: "worker", generation: 1, revision: 2, items: [], proposal: { state } })?.proposal?.state, state);
  }
});

test("approve calls the review port once and removes the card after persisted review", async () => {
  const calls: unknown[] = [];
  __setInvokeForTest((async (command: string, args: unknown) => {
    calls.push([command, args]);
    return { tab_id: "worker", generation: 1, revision: 4, items: [], proposal: { state: "approved" } };
  }) as never);
  const host = document.createElement("div"); const root = createRoot(host);
  try {
    await act(async () => root.render(<ChecklistProposalCard checklist={checklist} title="Mica" />));
    assert.equal(host.querySelectorAll("li").length, 2);
    await act(async () => button(host, "Approve").click());
    assert.deepEqual(calls, [["tab_todo_review", { tabId: "worker", request: { generation: 1, revision: 3 } }]]);
    assert.equal(host.textContent, "");
  } finally { await act(async () => root.unmount()); __setInvokeForTest(null); }
});

test("edit keeps criteria immutable, removes own items, and saves against the edit snapshot", async () => {
  const calls: Array<Record<string, unknown>> = [];
  __setInvokeForTest((async (_command: string, args: Record<string, unknown>) => {
    calls.push(args); throw new Error("checklist changed");
  }) as never);
  const host = document.createElement("div"); const root = createRoot(host);
  try {
    await act(async () => root.render(<ChecklistProposalCard checklist={checklist} title="Mica" />));
    await act(async () => button(host, "Edit").click());
    assert.equal(host.querySelectorAll('input[aria-label^="Plan item"]').length, 1);
    assert.equal(host.querySelector('button[aria-label="Remove Deliver result"]'), null);
    await act(async () => button(host, "Remove").click());
    await act(async () => root.render(<ChecklistProposalCard checklist={{ ...checklist, revision: 9 }} title="Mica" />));
    await act(async () => button(host, "Save amendment").click());
    assert.deepEqual(calls[0], { tabId: "worker", request: { generation: 1, revision: 3, items: [{ id: "criterion", content: "Deliver result", state: "in_progress" }] } });
    assert.ok(host.querySelector('[role="alert"]'));
    assert.ok(button(host, "Save amendment"));
  } finally { await act(async () => root.unmount()); __setInvokeForTest(null); }
});
