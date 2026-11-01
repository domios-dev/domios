import "./ChatComposer.composition.harness";
import assert from "node:assert/strict";
import { test } from "node:test";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { PtyModelChip } from "./PtyModelChip";
import { createMuxStateStore } from "../../store/multiplexer/selectorStore";
import { MuxStateStoreContext } from "../../store/multiplexer/context";
import type { ExtendedState } from "../../store/multiplexer/tree";
import type { ProviderDescriptor } from "../../lib/types";
import { __setTabModelApiForTest } from "../Sidebar/tabModelApi";

test("reasoning control switches the bound tab once and reports the chosen path", async () => {
  const state: ExtendedState = {
    workspaces: [], tabs: { fixture: { id: "fixture", pty_id: "pty", title: "Fixture", tab_type: "Terminal", cwd: null,
      agent_type: "provider", agent_provider_resolved_model: "vendor/model" } },
    teams: [], active_workspace_id: null, focused_tab_id: null, notifications: [], tabStatuses: {}, toasts: [], activeTabIds: {}, generation: 1, pendingCloses: [],
  };
  const provider = { id: "provider", model_switch: "live", available_models: [{ id: "vendor/model", display_name: "Model",
    reasoning_efforts: [{ id: "default", display_name: "Default" }, { id: "high", display_name: "High" }], default_reasoning_effort: "default" }] } as ProviderDescriptor;
  const calls: unknown[][] = [];
  __setTabModelApiForTest({ switchModel: async (...args) => { calls.push(args); return { mode_used: "live", model_confirmation: "observed" }; } });
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const store = createMuxStateStore(() => state);
  try {
    await act(async () => root.render(<MuxStateStoreContext.Provider value={store}>
      <PtyModelChip tabId="fixture" provider={provider} onError={(error) => { throw new Error(error); }} />
    </MuxStateStoreContext.Provider>));
    await act(async () => host.querySelector<HTMLButtonElement>('button[aria-label="Model and traits"]')!.click());
    const effort = Array.from(document.querySelectorAll<HTMLLabelElement>("label")).find((label) => label.textContent === "High")!.querySelector<HTMLInputElement>("input")!;
    await act(async () => effort.click());
    assert.deepEqual(calls, [["fixture", "vendor/model", "high"]]);
    assert.equal(host.querySelector('[role="status"]')?.textContent, "Live");
    await act(async () => document.querySelector('[role="dialog"]')!.dispatchEvent(new KeyboardEvent("keydown", { code: "Escape", bubbles: true })));
    assert.equal(document.querySelector('[role="dialog"]'), null);
    assert.equal(document.activeElement, host.querySelector('button[aria-label="Model and traits"]'));

  } finally {
    await act(async () => root.unmount()); host.remove(); __setTabModelApiForTest(null);
  }
});
