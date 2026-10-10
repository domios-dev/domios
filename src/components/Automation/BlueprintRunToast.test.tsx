import assert from "node:assert/strict";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { NotificationPanel } from "../Notifications/NotificationPanel";
import { NotificationToastContainer } from "../Notifications/NotificationToast";
import { OPEN_JOB_EVENT, type OpenJobDetail } from "../Jobs/openJobEvent";
import type { Notification } from "../../lib/types";

async function main() {
  const window = new Window({ url: "http://localhost" });
  for (const key of ["window", "document", "HTMLElement", "Element", "SVGElement", "Node", "navigator", "CustomEvent"]) Object.defineProperty(globalThis, key, { configurable: true, value: (window as unknown as Record<string, unknown>)[key] });
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  let frames = 0;
  Object.assign(globalThis, { requestAnimationFrame: () => ++frames, cancelAnimationFrame: () => undefined });
  const matchMedia = window.matchMedia.bind(window);
  window.matchMedia = (media) => { const query = matchMedia(media); Object.defineProperty(query, "matches", { value: true }); return query; };
  const requests: OpenJobDetail[] = [];
  window.addEventListener(OPEN_JOB_EVENT, (event) => requests.push((event as unknown as CustomEvent<OpenJobDetail>).detail));
  const notification: Notification = {
    id: "popup", title: "Blueprint Example ran: ready", tab_id: null, workspace_id: "workspace", body: null, timestamp: 10, severity: "Info", read: false,
    blueprint_run: { workspaceId: "workspace", teamId: "team", scenarioId: "scenario", count: 1, run: {
      key: "whole-run", at: 10, name: "Example", state: "completed", incomplete: false,
      events: [{ at: 10, node: "Observer", outcome: "ready", target: "Command", status: "completed", duration: 208, edgeId: "edge" }],
    } },
  };
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<NotificationToastContainer toasts={[notification]} onDismiss={() => {}} />));
    const view = [...host.querySelectorAll<HTMLButtonElement>("button")].find((button) => button.textContent === "View run");
    assert.ok(view);
    await act(async () => view.click());
    const dialog = document.querySelector('[role="dialog"]');
    assert.ok(dialog);
    assert.equal(dialog.querySelectorAll("ol > li").length, 2);
    assert.equal(frames, 0);
    const open = [...dialog.querySelectorAll<HTMLButtonElement>("button")].find((button) => button.textContent === "Open full graph");
    assert.ok(open);
    assert.equal(document.activeElement, open);
    await act(async () => open.dispatchEvent(new window.KeyboardEvent("keydown", { code: "Escape", bubbles: true }) as unknown as Event));
    assert.equal(document.querySelector('[role="dialog"]'), null);
    assert.equal(document.activeElement, view);
    await act(async () => view.click());
    const reopened = [...document.querySelectorAll<HTMLButtonElement>('[role="dialog"] button')].find((button) => button.textContent === "Open full graph");
    assert.ok(reopened);
    await act(async () => reopened.click());
    assert.deepEqual(requests, [{ workspaceId: "workspace", target: { teamId: "team", scenarioId: "scenario", runId: "whole-run" } }]);
    assert.equal(document.querySelector('[role="dialog"]'), null);
    await act(async () => root.render(<NotificationPanel notifications={[notification]} onMarkRead={() => {}} onMarkAllRead={() => {}} onDelete={() => {}} onClearAll={() => {}} onJumpToTab={() => {}} onClose={() => {}} />));
    const bellAction = [...host.querySelectorAll<HTMLButtonElement>("button")].find((button) => button.textContent === "Open run");
    assert.ok(bellAction);
    await act(async () => bellAction.click());
    assert.deepEqual(requests[1], { workspaceId: "workspace", target: { teamId: "team", scenarioId: "scenario", runId: "whole-run" } });
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
  console.log("Blueprint toast expands into a static reduced-motion path and opens the exact run");
}

void main().catch((error: unknown) => { console.error(error); process.exitCode = 1; });
