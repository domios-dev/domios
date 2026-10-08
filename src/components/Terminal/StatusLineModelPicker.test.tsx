import "./ChatComposer.composition.harness";
import assert from "node:assert/strict";
import { test } from "node:test";
import { act } from "react";
import { createRoot } from "react-dom/client";
import type { ProviderDescriptor } from "../../lib/types";
import { __setModelCatalogueRefreshApiForTest } from "../../lib/modelCatalogueRefresh";
import { __setTabModelApiForTest } from "../Sidebar/tabModelApi";
import { StatusLineModelPicker } from "./StatusLineModelPicker";

function descriptor(model_switch: ProviderDescriptor["model_switch"]): ProviderDescriptor {
  return {
    id: "vendor",
    display_name: "Vendor",
    model_switch,
    available_models: [
      { id: "fast", display_name: "Fast", reasoning_efforts: [] },
      {
        id: "deep",
        display_name: "Deep",
        reasoning_efforts: [{ id: "low", display_name: "Low" }, { id: "high", display_name: "High" }],
        default_reasoning_effort: "high",
      },
    ],
  } as ProviderDescriptor;
}

async function mount(provider: ProviderDescriptor | undefined) {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () => root.render(
    <StatusLineModelPicker
      tabId="tab"
      descriptor={provider}
      facts={{ hasSession: true, protected: false }}
      label="Vendor · deep · high"
      modelCandidates={["deep"]}
      currentEffort="high"
    />,
  ));
  return {
    host,
    trigger: () => host.querySelector<HTMLButtonElement>('[data-testid="status-model-trigger"]'),
    picker: () => document.body.querySelector<HTMLElement>('[data-testid="status-model-picker"]'),
    option: (name: string) => Array.from(document.body.querySelectorAll<HTMLElement>('[role="option"]'))
      .find((option) => option.textContent?.startsWith(name))!,
    effortTabs: () => Array.from(document.body.querySelectorAll<HTMLElement>('[aria-label="Reasoning"] [role="tab"]'))
      .map((tab) => tab.getAttribute("data-value")),
    async unmount() {
      await act(async () => root.unmount());
      host.remove();
    },
  };
}

function recordSwitches() {
  const calls: unknown[][] = [];
  __setTabModelApiForTest({
    switchModel: async (...args) => {
      calls.push(args);
      return { mode_used: "next_turn", model_confirmation: "pending" };
    },
  });
  return calls;
}

test("choosing a model switches the tab once and closes the picker", async () => {
  const calls = recordSwitches();
  const view = await mount(descriptor("next_turn"));
  try {
    await act(async () => view.trigger()!.click());
    assert.ok(view.picker());
    assert.equal(view.trigger()!.getAttribute("aria-expanded"), "true");
    assert.deepEqual(view.effortTabs(), ["low", "high"]);
    await act(async () => view.option("Fast").click());
    assert.deepEqual(calls, [["tab", "fast", undefined]]);
    assert.equal(view.picker(), null);
    assert.ok(view.host.querySelector('[role="status"]'));
  } finally {
    await view.unmount();
    __setTabModelApiForTest(null);
  }
});

test("restarting providers confirm first and the effort list follows the staged model", async () => {
  const calls = recordSwitches();
  const view = await mount(descriptor("restart_resume"));
  try {
    await act(async () => view.trigger()!.click());
    await act(async () => view.option("Fast").click());
    assert.deepEqual(calls, []);
    assert.deepEqual(view.effortTabs(), []);
    assert.ok(document.body.querySelector('[aria-label="Confirm restart"]'));
    await act(async () => view.option("Deep").click());
    assert.deepEqual(view.effortTabs(), ["low", "high"]);
    const low = document.body.querySelector<HTMLElement>('[aria-label="Reasoning"] [data-value="low"]')!;
    await act(async () => low.click());
    assert.deepEqual(calls, []);
    const confirm = Array.from(document.body.querySelectorAll<HTMLButtonElement>('[aria-label="Confirm restart"] button'))
      .find((button) => button.textContent === "Restart and switch")!;
    await act(async () => confirm.click());
    assert.deepEqual(calls, [["tab", "deep", "low"]]);
  } finally {
    await view.unmount();
    __setTabModelApiForTest(null);
  }
});

test("a provider without live switching renders plain text with a reason", async () => {
  const calls = recordSwitches();
  const view = await mount(descriptor("unsupported"));
  try {
    assert.equal(view.trigger(), null);
    assert.equal(view.host.querySelector("button"), null);
    const plain = view.host.querySelector<HTMLElement>("[title]")!;
    assert.ok(plain.getAttribute("title"));
    assert.equal(plain.textContent, "Vendor · deep · high");
    assert.deepEqual(calls, []);
  } finally {
    await view.unmount();
    __setTabModelApiForTest(null);
  }
});

test("refresh asks the provider's catalogue once and escape returns focus", async () => {
  const refreshed: string[] = [];
  __setModelCatalogueRefreshApiForTest({ refresh: async (id) => { refreshed.push(id); return true; } });
  const view = await mount(descriptor("live"));
  try {
    await act(async () => view.trigger()!.click());
    await act(async () => document.body.querySelector<HTMLButtonElement>('button[aria-label="Refresh models"]')!.click());
    assert.deepEqual(refreshed, ["vendor"]);
    assert.equal(document.body.querySelector('[role="alert"]'), null);
    await act(async () => view.picker()!.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })));
    assert.equal(view.picker(), null);
    assert.equal(document.activeElement, view.trigger());
  } finally {
    await view.unmount();
    __setModelCatalogueRefreshApiForTest(null);
  }
});

test("an open picker follows a catalogue change without polling", async () => {
  const { setAgentDescriptors } = await import("../../lib/agentDescriptorCache");
  const { TabStatusLineModelPicker } = await import("./StatusLineModelPicker");
  const base = descriptor("live");
  setAgentDescriptors([{ ...base, catalogue_version: 1 }]);
  const tab = { id: "tab", pty_id: "pty", agent_type: "vendor", agent_provider_resolved_model: "deep" } as unknown as import("../../lib/types").Tab;
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const options = () => Array.from(document.body.querySelectorAll('[role="option"]')).map((option) => option.textContent);
  try {
    await act(async () => root.render(<TabStatusLineModelPicker tab={tab} label="Vendor · deep" />));
    await act(async () => host.querySelector<HTMLButtonElement>('[data-testid="status-model-trigger"]')!.click());
    assert.equal(options().length, 2);
    await act(async () => setAgentDescriptors([{
      ...base,
      catalogue_version: 2,
      available_models: [...base.available_models, { id: "newest", display_name: "Newest" }],
    }]));
    assert.equal(options().length, 3);
    assert.ok(options().some((text) => text?.startsWith("Newest")));
  } finally {
    await act(async () => root.unmount());
    host.remove();
    setAgentDescriptors([]);
  }
});
