import { tabActivity } from "../../lib/tabActivity";
import "./ChatComposer.composition.harness";
import assert from "node:assert/strict";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { ManagedTabBar } from "./ManagedTabBar";

import { test } from "vitest";

test("managed ownership wires Stop and releases input after manager closure", async () => {
  const managed = { manager_tab_id: "manager", manager_name: "Scout", manager_closed: false };
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  let stopped = 0;
  await act(async () => root.render(<ManagedTabBar activity={tabActivity({ agent_session_id: "bound-session", agent_state: { state: "working" } })} managed={managed} onStop={() => stopped++} />));
  assert.equal(host.querySelector('[role="status"]')?.getAttribute("aria-label"), "Managed by Scout");
  const stop = host.querySelector<HTMLButtonElement>('button[aria-label="Stop generation"]');
  assert.ok(stop);
  await act(async () => stop.click());
  assert.equal(stopped, 1);
  await act(async () => root.render(<ManagedTabBar activity={tabActivity({ agent_state: { state: "idle" } })} managed={managed} onStop={() => stopped++} />));
  assert.equal(host.querySelector('button[aria-label="Stop generation"]'), null);
  await act(async () => root.render(<ManagedTabBar activity={tabActivity({ agent_session_id: "bound-session", agent_state: { state: "working" } })} managed={{ ...managed, manager_closed: true }} onStop={() => stopped++} />));
  assert.ok(host.querySelector('[role="status"]'));
  assert.equal(host.querySelector('button'), null);
  await act(async () => root.render(<ManagedTabBar activity={tabActivity({ agent_session_id: "bound-session", agent_state: { state: "working" } })} managed={null} onStop={() => stopped++} />));
  assert.equal(host.querySelector('[role="status"]'), null);
  await act(async () => root.unmount());
  host.remove();
});
