import type { RuntimeServiceTierSelection as ServiceTierSelection } from "../../../packages/plugin-sdk/src/agentRuntime";
import "./ChatComposer.composition.harness";
import assert from "node:assert/strict";
import { test } from "vitest";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { TransportError } from "../../lib/transportClient";
import type { Notification, ProviderDescriptor, Tab } from "../../lib/types";
import { __setModelCatalogueRefreshApiForTest } from "../../lib/modelCatalogueRefresh";
import { NotificationToastContainer } from "../Notifications/NotificationToast";
import { __setTabModelApiForTest } from "../Sidebar/tabModelApi";
import { readPickerShowAll, writePickerShowAll } from "./statusLinePickerPreference";
import { StatusLineModelPicker } from "./StatusLineModelPicker";
import { MultiplexerActionsContext } from "../../store/multiplexer/context";

type RaisedToast = { title: string; body: string | null; local_actions?: { label: string; run: () => void }[] };
const toasts: RaisedToast[] = [];
const toastActions = { upsertToast: (toast: RaisedToast) => { toasts.push(toast); } } as unknown as Parameters<typeof MultiplexerActionsContext.Provider>[0]["value"];

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

async function mount(provider: ProviderDescriptor | undefined, currentServiceTier?: ServiceTierSelection, serviceTierAvailability?: Record<string, readonly import("../../../packages/plugin-sdk/src/agentRuntime").RuntimeServiceTier[]>, currentRuntimeMode?: string, runtimeControls?: import("@codeterm/plugin-sdk").RuntimeSettings) {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  toasts.length = 0;
  const render = (pendingSelection?: { model: string; effort: string | null; confirmation_pending?: boolean }) => root.render(
    <MultiplexerActionsContext.Provider value={toastActions}>
    <StatusLineModelPicker
      tabId="tab"
      descriptor={provider}
      runtimeControls={runtimeControls}
      runtimeIdentity={runtimeControls ? {agentName:"Wire Agent",binaryVersion:"0.0.1"} : undefined}
      facts={{ hasSession: true }}
      label="Vendor · deep · high"
      modelCandidates={["deep"]}
      currentEffort="high"
      currentServiceTier={currentServiceTier}
      currentRuntimeMode={currentRuntimeMode}
      serviceTierAvailability={serviceTierAvailability}
      pendingSelection={pendingSelection}
    />
    </MultiplexerActionsContext.Provider>,
  );
  await act(async () => render());
  return {
    host,
    async setPendingSelection(selection?: { model: string; effort: string | null; confirmation_pending?: boolean }) {
      await act(async () => render(selection));
    },
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

function button(name: string): HTMLButtonElement {
  const found = Array.from(document.body.querySelectorAll<HTMLButtonElement>("button"))
    .find((entry) => entry.textContent === name);
  assert.ok(found);
  return found;
}

async function openSetting(label: string) {
  const trigger = document.body.querySelector<HTMLButtonElement>(`button[aria-label^="${label}:"]`)!;
  assert.ok(trigger);
  await act(async () => trigger.dispatchEvent(new Event("pointerdown", { bubbles: true, cancelable: true })));
}
function settingOption(label: string) {
  return Array.from(document.body.querySelectorAll<HTMLButtonElement>('[role="menuitemradio"]')).find(item => item.textContent?.trim() === label);
}
async function chooseSetting(label: string, value: string) {
  await openSetting(label);
  const item = settingOption(value);
  assert.ok(item);
  await act(async () => item.click());
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

test("a busy apply closes the picker and cancel or Escape clears its staged request", async () => {
  const requests: string[] = [];
  __setTabModelApiForTest({
    switchModel: async (_tab, model) => {
      requests.push(model);
      return { mode_used: "live_command", model_confirmation: "pending", staged: true };
    },
    cancelStagedModel: async (tab) => { requests.push(`cancel:${tab}`); },
  });
  const view = await mount(descriptor("live_command"));
  try {
    await act(async () => view.trigger()!.click());
    await act(async () => view.option("Fast").click());
    await act(async () => button("Apply").click());
    assert.equal(view.picker(), null);
    await view.setPendingSelection({ model: "fast", effort: null });
    assert.match(view.trigger()!.getAttribute("aria-label")!, /Fast.*after this turn/);
    await act(async () => view.trigger()!.click());
    await act(async () => button("Cancel").click());
    assert.deepEqual(requests, ["fast", "cancel:tab"]);
    assert.equal(view.picker(), null);
    await view.setPendingSelection();
    assert.doesNotMatch(view.trigger()!.getAttribute("aria-label")!, /after this turn/);
    await act(async () => view.trigger()!.click());
    await act(async () => view.picker()!.dispatchEvent(new KeyboardEvent("keydown", { code: "Escape", bubbles: true })));
    assert.deepEqual(requests, ["fast", "cancel:tab", "cancel:tab"]);
  } finally {
    await view.unmount();
    __setTabModelApiForTest(null);
  }
});

test("only the model capsule is interactive and the harness rides inside it as a decorative glyph", async () => {
  const view = await mount(descriptor("next_turn"));
  try {
    const trigger = view.trigger()!;
    assert.equal(view.host.querySelectorAll("button").length, 1);
    assert.equal(trigger.querySelector(".ct-status-identity__text")?.textContent, "Deep · high");
    assert.equal(trigger.getAttribute("aria-label"), "Change model, currently Deep · high");
    const glyph = trigger.querySelector('[data-testid="status-harness-glyph"]');
    assert.ok(glyph);
    assert.equal(glyph.getAttribute("aria-hidden"), "true");
    assert.equal(glyph.getAttribute("title"), "Vendor");
    assert.equal(Array.from(view.host.querySelectorAll("span")).some((entry) => entry.textContent === "Vendor"), false);
  } finally {
    await view.unmount();
  }
});

test("the curated picker toggle reveals all models and persists when reopened", async () => {
  writePickerShowAll("vendor", false);
  const base = descriptor("next_turn");
  base.available_models[0].primary = true;
  const view = await mount(base);
  try {
    await act(async () => view.trigger()!.click());
    assert.equal(document.body.querySelectorAll('[role="option"]').length, 1);
    await act(async () => button("Show all models").click());
    assert.equal(document.body.querySelectorAll('[role="option"]').length, 2);
    assert.equal(readPickerShowAll("vendor"), true);
    assert.equal(readPickerShowAll("different-provider"), false);
    await act(async () => button("Cancel").click());
    await act(async () => view.trigger()!.click());
    assert.equal(document.body.querySelectorAll('[role="option"]').length, 2);
    assert.equal(button("Show primary models").getAttribute("aria-pressed"), "true");
    await act(async () => button("Show primary models").click());
    assert.equal(document.body.querySelectorAll('[role="option"]').length, 1);
    assert.equal(readPickerShowAll("vendor"), false);
  } finally {
    await view.unmount();
    writePickerShowAll("vendor", false);
  }
});

test("picker preferences tolerate blocked browser storage", () => {
  const original = Object.getOwnPropertyDescriptor(window, "localStorage");
  Object.defineProperty(window, "localStorage", { configurable: true, get() { throw new Error("denied"); } });
  try {
    assert.equal(readPickerShowAll("vendor"), false);
    assert.doesNotThrow(() => writePickerShowAll("vendor", true));
  } finally {
    if (original) Object.defineProperty(window, "localStorage", original);
    else Reflect.deleteProperty(window, "localStorage");
  }
});

test("model rows stage a choice and Apply switches once and closes the picker", async () => {
  const calls = recordSwitches();
  const view = await mount(descriptor("next_turn"));
  try {
    await act(async () => view.trigger()!.click());
    assert.ok(view.picker());
    assert.equal(view.trigger()!.getAttribute("aria-expanded"), "true");
    assert.deepEqual(view.effortTabs(), ["low", "high"]);
    await act(async () => view.option("Fast").click());
    assert.deepEqual(calls, []);
    await act(async () => button("Apply").click());
    assert.deepEqual(calls, [["tab", "fast", undefined]]);
    assert.equal(view.picker(), null);
    assert.equal(view.host.querySelector('[role="status"]'), null);
    assert.equal(view.trigger()!.textContent, "Fast");
  } finally {
    await view.unmount();
    __setTabModelApiForTest(null);
  }
});

test("declared runtime modes show the bound value and apply with other staged choices", async () => {
  const calls = recordSwitches();
  const provider = descriptor("live");
  provider.agent_runtime = {
    runtimeModes: [
      { id: "auto", displayName: "Auto" },
      { id: "full-access", displayName: "Full access" },
    ],
  };
  const view = await mount(provider, undefined, undefined, "auto");
  try {
    await act(async () => view.trigger()!.click());
    assert.ok(document.body.querySelector('button[aria-label="Runtime mode: Auto"]'));
    await act(async () => view.option("Fast").click());
    await chooseSetting("Runtime mode", "Full access");
    assert.deepEqual(calls, []);
    await act(async () => button("Apply").click());
    assert.deepEqual(calls, [["tab", "fast", undefined, "full-access", undefined]]);
  } finally {
    await view.unmount();
    __setTabModelApiForTest(null);
  }
});

test("runtime mode control is absent without declared capability modes", async () => {
  const view = await mount(descriptor("live"));
  try {
    await act(async () => view.trigger()!.click());
    assert.equal(document.body.querySelector('button[aria-label^="Runtime mode:"]'), null);
  } finally {
    await view.unmount();
  }
});

test("live commands switch immediately without restart confirmation", async () => {
  const calls = recordSwitches();
  const view = await mount(descriptor("live_command"));
  try {
    await act(async () => view.trigger()!.click());
    await act(async () => view.option("Fast").click());
    assert.deepEqual(calls, []);
    await act(async () => button("Apply").click());
    assert.deepEqual(calls, [["tab", "fast", undefined]]);
    assert.equal(document.body.querySelector('[aria-label="Confirm restart"]'), null);
    assert.equal(view.picker(), null);
  } finally {
    await view.unmount();
    __setTabModelApiForTest(null);
  }
});

test("a failed live command closes the picker and offers restart through a toast", async () => {
  const fallback: unknown[][] = [];
  __setTabModelApiForTest({
    switchModel: async () => { throw new Error("opaque failure"); },
    restartAndSwitchModel: async (...args) => { fallback.push(args); },
  });
  const view = await mount(descriptor("live_command"));
  try {
    await act(async () => view.trigger()!.click());
    await act(async () => view.option("Fast").click());
    await act(async () => button("Apply").click());
    assert.equal(view.picker(), null);
    assert.equal(document.body.querySelector('[role="alert"]'), null);
    assert.equal(toasts.length, 1);
    assert.equal(toasts[0].title, "Model switch did not apply");
    assert.equal(toasts[0].body, "opaque failure");
    assert.deepEqual(fallback, []);
    const restart = toasts[0].local_actions?.find((action) => action.label === "Restart and resume");
    assert.ok(restart);
    await act(async () => restart.run());
    assert.deepEqual(fallback, [["tab", "fast", undefined, undefined]]);
  } finally {
    await view.unmount();
    __setTabModelApiForTest(null);
  }
});

test("runtime refusal toast shows the provider reason without restart", async () => {
  __setTabModelApiForTest({
    switchModel: async () => { throw new TransportError(409, "/api/tabs/owned/agent/model", JSON.stringify({
      reason: "runtime_control_refused", error: "Provider refused: settings unavailable",
    })); },
  });
  const view = await mount({ ...descriptor("next_turn"), transport: "chat_backend" });
  try {
    await act(async () => view.trigger()!.click());
    await act(async () => view.option("Fast").click());
    await act(async () => button("Apply").click());
    assert.equal(toasts.length, 1);
    assert.equal(toasts[0].body, "Provider refused: settings unavailable");
    assert.equal(toasts[0].local_actions, undefined);
    assert.equal(view.picker(), null);
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
    assert.equal(document.body.querySelector('[aria-label="Confirm restart"]'), null);
    await act(async () => view.option("Deep").click());
    assert.deepEqual(view.effortTabs(), ["low", "high"]);
    const low = document.body.querySelector<HTMLElement>('[aria-label="Reasoning"] [data-value="low"]')!;
    await act(async () => low.click());
    assert.deepEqual(calls, []);
    assert.equal(document.body.querySelector('[aria-label="Confirm restart"]'), null);
    await act(async () => button("Apply").click());
    assert.ok(document.body.querySelector('[aria-label="Confirm restart"]'));
    const confirm = button("Restart and switch");
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
    assert.equal(plain.textContent, "Deep · high");
    assert.deepEqual(calls, []);
  } finally {
    await view.unmount();
    __setTabModelApiForTest(null);
  }
});

test("refresh asks the provider's catalogue once and escape returns focus", async () => {
  const refreshed: string[] = [];
  __setModelCatalogueRefreshApiForTest({ refresh: async (id) => { refreshed.push(id); return { refreshed: true }; } });
  const view = await mount(descriptor("live"));
  try {
    await act(async () => view.trigger()!.click());
    await act(async () => document.body.querySelector<HTMLButtonElement>('button[aria-label="Refresh models"]')!.click());
    assert.deepEqual(refreshed, ["vendor"]);
    assert.equal(document.body.querySelector('[role="alert"]'), null);
    await act(async () => view.picker()!.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", code: "Escape", bubbles: true })));
    assert.equal(view.picker(), null);
    assert.equal(document.activeElement, view.trigger());
  } finally {
    await view.unmount();
    __setModelCatalogueRefreshApiForTest(null);
  }
});

test("a failed refresh raises a copyable toast and keeps the picker usable", async () => {
  __setModelCatalogueRefreshApiForTest({ refresh: async () => ({ refreshed: false, reason: "opaque-reason" }) });
  const copied: string[] = [];
  const clipboard = Object.getOwnPropertyDescriptor(navigator, "clipboard");
  Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText: async (text: string) => { copied.push(text); } } });
  toasts.length = 0;
  const view = await mount(descriptor("live"));
  try {
    await act(async () => view.trigger()!.click());
    await act(async () => document.body.querySelector<HTMLButtonElement>('button[aria-label="Refresh models"]')!.click());
    assert.equal(document.body.querySelector('[role="alert"]'), null);
    assert.ok(view.picker()!.querySelectorAll('[role="option"]').length > 0);
    assert.equal(toasts.length, 1);
    assert.equal(toasts[0].body, null);
    assert.doesNotMatch(toasts[0].title, /opaque-reason/);
    const toastHost = document.createElement("div");
    document.body.append(toastHost);
    const toastRoot = createRoot(toastHost);
    await act(async () => toastRoot.render(
      <NotificationToastContainer toasts={[{ ...(toasts[0] as unknown as Notification), timestamp: 0, read: false }]} onDismiss={() => undefined} />,
    ));
    const copy = Array.from(document.body.querySelectorAll<HTMLButtonElement>("button")).filter((button) => button.textContent === "Copy");
    assert.equal(copy.length, 1);
    await act(async () => copy[0].click());
    assert.deepEqual(copied, ["opaque-reason"]);
    await act(async () => toastRoot.unmount());
    toastHost.remove();
  } finally {
    await view.unmount();
    __setModelCatalogueRefreshApiForTest(null);
    if (clipboard) Object.defineProperty(navigator, "clipboard", clipboard);
    else delete (navigator as { clipboard?: unknown }).clipboard;
    toasts.length = 0;
  }
});

test("tab picker retains the observed model when the streaming label ends", async () => {
  const { setAgentDescriptors } = await import("../../lib/agentDescriptorCache");
  const { TabStatusLineModelPicker } = await import("./StatusLineModelPicker");
  setAgentDescriptors([descriptor("live")]);
  const tab = {
    id: "tab", pty_id: "pty", agent_type: "vendor",
    agent_requested_model: "deep", agent_provider_resolved_model: "deep",
    agent_session_model: "vendor/actual",
  } as unknown as import("../../lib/types").Tab;
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    for (const label of ["Vendor · vendor/actual", "Vendor · Deep"]) {
      await act(async () => root.render(<TabStatusLineModelPicker tab={{ ...tab }} label={label} />));
      assert.equal(host.querySelector('button[aria-haspopup="dialog"]')?.getAttribute("aria-label"),
        "Change model, currently vendor/actual");
    }
    await act(async () => root.render(<TabStatusLineModelPicker tab={{ ...tab,
      pending_model_switch_observation: { launched_model: "fast" },
    }} label="Vendor" />));
    assert.equal(host.querySelector('button[aria-haspopup="dialog"]')?.getAttribute("aria-label"),
      "Change model, currently Fast · after this turn");
    await act(async () => root.render(<TabStatusLineModelPicker tab={{ ...tab,
      agent_session_model: "fast",
    }} label="Vendor" />));
    assert.equal(host.querySelector('button[aria-haspopup="dialog"]')?.getAttribute("aria-label"),
      "Change model, currently Fast");
  } finally {
    await act(async () => root.unmount());
    host.remove();
    setAgentDescriptors([]);
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

for (const cancel of ["Escape", "Cancel"] as const) {
  test(`${cancel} discards staged model and effort and reopening seeds the current values`, async () => {
    const calls = recordSwitches();
    const view = await mount(descriptor("next_turn"));
    try {
      await act(async () => view.trigger()!.click());
      assert.equal(button("Apply").disabled, true);
      await act(async () => view.option("Fast").click());
      await act(async () => view.option("Deep").click());
      assert.equal(button("Apply").disabled, true);
      await act(async () => button("Low").click());
      assert.equal(button("Apply").disabled, false);
      assert.deepEqual(calls, []);
      await act(async () => {
        if (cancel === "Cancel") button("Cancel").click();
        else view.picker()!.dispatchEvent(new KeyboardEvent("keydown", { code: "Escape", bubbles: true }));
      });
      assert.equal(view.picker(), null);
      assert.deepEqual(calls, []);
      await act(async () => view.trigger()!.click());
      assert.equal(view.option("Deep").getAttribute("aria-selected"), "true");
      assert.equal(button("High").getAttribute("aria-selected"), "true");
      assert.equal(button("Apply").disabled, true);
    } finally {
      await view.unmount();
      __setTabModelApiForTest(null);
    }
  });
}

test("Enter commits the staged effort once and shows it in the status line", async () => {
  const calls = recordSwitches();
  const view = await mount(descriptor("next_turn"));
  try {
    await act(async () => view.trigger()!.click());
    await act(async () => button("Low").click());
    assert.deepEqual(calls, []);
    await act(async () => view.option("Deep").dispatchEvent(new KeyboardEvent("keydown", { code: "Enter", bubbles: true })));
    assert.deepEqual(calls, [["tab", "deep", "low"]]);
    assert.equal(view.picker(), null);
    assert.equal(view.trigger()!.textContent, "Deep · low");
    await act(async () => view.trigger()!.click());
    assert.equal(button("Apply").disabled, true);
    assert.equal(button("Low").getAttribute("aria-selected"), "true");
  } finally {
    await view.unmount();
    __setTabModelApiForTest(null);
  }
});

for (const fails of [false, true]) {
  test(`Apply closes the picker, shows Applying with the draft, and ${fails ? "rolls back on failure with a toast" : "retains it on success"}`, async () => {
    let resolve!: () => void;
    let reject!: (error: Error) => void;
    const response = new Promise<void>((yes, no) => { resolve = yes; reject = no; });
    const calls: unknown[][] = [];
    __setTabModelApiForTest({ switchModel: async (...args) => { calls.push(args); await response; } });
    const view = await mount(descriptor("next_turn"));
    try {
      await act(async () => view.trigger()!.click());
      await act(async () => view.option("Fast").click());
      await act(async () => view.option("Deep").click());
      await act(async () => button("Low").click());
      await act(async () => button("Apply").click());
      assert.equal(view.trigger()!.textContent, "Deep · low");
      assert.equal(view.picker(), null);
      assert.equal(view.host.querySelector('[role="status"]')?.textContent, "Applying…");
      assert.deepEqual(calls, [["tab", "deep", "low"]]);
      await act(async () => { if (fails) reject(new Error("refused")); else resolve(); });
      if (fails) {
        assert.equal(view.trigger()!.textContent, "Deep · high");
        assert.equal(view.picker(), null);
        assert.equal(toasts.length, 1);
      } else {
        assert.equal(view.trigger()!.textContent, "Deep · low");
        assert.equal(view.picker(), null);
      }
      assert.equal(view.host.querySelector('[role="status"]'), null);
    } finally {
      await view.unmount();
      __setTabModelApiForTest(null);
    }
  });
}

test("the protected General Agent exposes the shared model picker and switch handler", async () => {
  const { setAgentDescriptors } = await import("../../lib/agentDescriptorCache");
  const { TabStatusLineModelPicker } = await import("./StatusLineModelPicker");
  const calls = recordSwitches();
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const tab = {
    id: "general-main", title: "main", protected: true, role: "General",
    pty_id: "pty", agent_type: "ga-capable", agent_session_model: "deep",
  } as unknown as import("../../lib/types").Tab;
  try {
    for (const mode of ["live_command", "next_turn", "restart_resume"] as const) {
      await act(async () => setAgentDescriptors([{ ...descriptor(mode), id: "ga-capable" }]));
      await act(async () => root.render(<TabStatusLineModelPicker key={mode} tab={tab} label="Vendor · Deep" />));
      const trigger = host.querySelector<HTMLButtonElement>('button[aria-label="Change model, currently Deep"]');
      assert.ok(trigger);
      await act(async () => trigger.click());
      const option = Array.from(document.body.querySelectorAll<HTMLElement>('[role="option"]'))
        .find((entry) => entry.textContent?.startsWith("Fast"));
      assert.ok(option);
      await act(async () => option.click());
      await act(async () => button("Apply").click());
      if (mode === "restart_resume") await act(async () => button("Restart and switch").click());
    }
    assert.deepEqual(calls, Array.from({ length: 3 }, () => ["general-main", "fast", undefined]));
  } finally {
    await act(async () => root.unmount());
    host.remove();
    setAgentDescriptors([]);
    __setTabModelApiForTest(null);
  }
});


test("speed selections apply once and unavailable tiers are disabled", async () => {
  const calls = recordSwitches();
  const provider = {...descriptor("next_turn"),agent_runtime:{serviceTiers:[{id:"priority",name:"Fast",description:""},{id:"ultrafast",name:"Ultrafast",description:"",unavailableReason:"Not available on your plan"}]}};
  const view = await mount(provider,{id:null});
  try {
    await act(async () => view.trigger()!.click());
    await openSetting("Speed");
    assert.ok(settingOption("Default"));
    assert.equal(settingOption("Ultrafast")?.disabled, true);
    await act(async () => settingOption("Fast")!.click());
    assert.equal(calls.length,0);
    await act(async () => button("Apply").click());
    assert.deepEqual(calls,[["tab","deep","high",undefined,{id:"priority"}]]);
  } finally { await view.unmount(); __setTabModelApiForTest(null); }
});

test("providers without service tiers have no speed control", async () => {
  const view = await mount(descriptor("next_turn"));
  try {
    await act(async () => view.trigger()!.click());
    assert.equal(document.body.querySelector('button[aria-label^="Speed:"]'),null);
  } finally { await view.unmount(); }
});

test("Advanced shows a chevron, toggles in place, and leaves Enter to the summary without applying", async () => {
  const calls = recordSwitches();
  const provider = { ...descriptor("next_turn"), agent_runtime: {
    runtimeModes: [{ id: "full-access", displayName: "Full access" }],
  } } as ProviderDescriptor;
  const view = await mount(provider);
  try {
    await act(async () => view.trigger()!.click());
    await act(async () => view.option("Fast").click());
    assert.equal(button("Apply").disabled, false);
    const details = view.picker()!.querySelector("details")!;
    const summary = details.querySelector("summary")!;
    assert.ok(summary.querySelector('svg[aria-hidden="true"]'));
    assert.equal(details.open, false);
    await act(async () => summary.click());
    assert.equal(details.open, true);
    summary.focus();
    const enter = new KeyboardEvent("keydown", { key: "Enter", code: "Enter", bubbles: true, cancelable: true });
    await act(async () => summary.dispatchEvent(enter));
    assert.equal(enter.defaultPrevented, false);
    assert.deepEqual(calls, []);
    assert.ok(view.picker());
    await act(async () => summary.click());
    assert.equal(details.open, false);
  } finally {
    await view.unmount();
    __setTabModelApiForTest(null);
  }
});

test("an account catalogue can enable Ultrafast for its selected model", async () => {
  const calls = recordSwitches();
  const provider = {...descriptor("next_turn"),agent_runtime:{serviceTiers:[{id:"ultrafast",name:"Ultrafast",description:"",unavailableReason:"unknown"}]}};
  const view = await mount(provider,{id:null},{deep:[{id:"ultrafast",name:"Ultrafast",description:""}]});
  try {
    await act(async () => view.trigger()!.click());
    await openSetting("Speed");
    assert.equal(settingOption("Ultrafast")?.disabled,false);
    await act(async () => settingOption("Ultrafast")!.click());
    await act(async () => button("Apply").click());
    assert.deepEqual(calls,[["tab","deep","high",undefined,{id:"ultrafast"}]]);
  } finally { await view.unmount(); __setTabModelApiForTest(null); }
});

test("model chip displays provider-confirmed tier names", async () => {
  const provider = {...descriptor("next_turn"),agent_runtime:{serviceTiers:[{id:"priority",name:"Fast",description:""},{id:"ultrafast",name:"Ultrafast",description:""}]}};
  for (const id of [null,"priority","ultrafast"]) {
    const view = await mount(provider,{id});
    try {
      assert.equal(view.trigger()!.textContent?.includes(" · fast"),id === "priority");
      assert.equal(view.trigger()!.textContent?.includes(" · ultrafast"),id === "ultrafast");
    } finally { await view.unmount(); }
  }
});

test("sent switches expose pending confirmation and cannot cancel an already written command", async () => {
 const calls: string[] = [];
 __setTabModelApiForTest({switchModel: async () => ({mode_used:"live_command",model_confirmation:"pending"}), cancelStagedModel:async () => {calls.push("cancel");}});
 const view=await mount(descriptor("live_command"));
 try {
  await view.setPendingSelection({model:"fast",effort:null,confirmation_pending:true});
  assert.match(view.trigger()!.getAttribute("aria-label")!, /Switch sent; confirms when this turn ends/);
  assert.equal(toasts.length,0);
  await act(async () => view.trigger()!.click());
  await act(async () => button("Cancel").click());
  assert.deepEqual(calls,[]);
 } finally {await view.unmount();__setTabModelApiForTest(null);}
});


test("catalogue-free runtime lists only the bound agent model declaration", async () => {
  const calls = recordSwitches();
  const provider = {...descriptor("unsupported"), available_models:[], agent_runtime:{}, transport:"chat_backend"} as ProviderDescriptor;
  const view = await mount(provider, undefined, undefined, undefined, {configOptions:[
    {id:"choice",name:"Engine",category:"model",currentValue:"wire-one",options:[{value:"wire-one",name:"First declared"},{value:"wire-two",name:"Second declared"}]},
    {id:"other",name:"Other",currentValue:"unrelated",options:[{value:"unrelated",name:"Unrelated"}]},
  ]});
  try {
    assert.equal(view.trigger()!.textContent, "First declared · high");
    await act(async () => view.trigger()!.click());
    assert.deepEqual(Array.from(document.body.querySelectorAll('[role="option"]')).map(row => row.textContent?.includes("declared")), [true,true]);
    assert.ok(document.body.querySelector('[aria-label="Refresh models"]'));
    assert.equal(view.option("Deep"), undefined);
    assert.equal(view.option("Unrelated"), undefined);
    // Applying a declared model goes through the runtime controls API, never the catalogue switch
    // (runtimeModelControls.test.tsx covers that path).
    assert.deepEqual(calls, []);
  } finally { await view.unmount(); __setTabModelApiForTest(null); }
});

test("catalogue-free runtime keeps unclassified choices inside a neutral model bubble", async () => {
  const provider = {...descriptor("unsupported"), available_models:[], agent_runtime:{}} as ProviderDescriptor;
  const view = await mount(provider, undefined, undefined, undefined, {configOptions:[
    {id:"model",name:"Model",currentValue:"undeclared",options:[{value:"undeclared",name:"Undeclared"}]},
  ]});
  try {
    assert.equal(view.trigger()?.textContent, "deep · high");
    assert.doesNotMatch(view.host.textContent!, /Undeclared|Default/);
    await act(async () => view.trigger()!.click());
    assert.ok(document.body.querySelector('button[aria-label="Model: Undeclared"]'));
    assert.equal(view.option("Undeclared"), undefined);
  } finally { await view.unmount(); }
});


test("an unreported model uses the harness name instead of borrowing the runtime mode", async () => {
  const provider = {...descriptor("unsupported"), available_models:[], agent_runtime:{hostApprovals:true}} as unknown as ProviderDescriptor;
  const chip = async (runtimeState: "degraded" | "unloaded" | "loaded", config: Record<string, unknown>) => {
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    const tab = {id:"tab", agent_runtime_state:{state:runtimeState, epoch:1}, agent_runtime_config:config} as unknown as Tab;
    await act(async () => root.render(<MultiplexerActionsContext.Provider value={toastActions}>
      <StatusLineModelPicker tab={tab} tabId="tab" descriptor={provider} runtimeControls={{}} facts={{ hasSession: true }}
        label="Vendor" modelCandidates={[]} currentEffort={undefined} />
    </MultiplexerActionsContext.Provider>));
    const text = host.querySelector(".ct-status-identity__text")?.textContent;
    await act(async () => root.unmount());
    host.remove();
    return text;
  };
  assert.equal(await chip("degraded", {sessionMode:"opaque-chosen", fullAccessMode:"opaque-declared"}), "Vendor");
  assert.equal(await chip("unloaded", {fullAccessMode:"opaque-declared"}), "Vendor");
  assert.equal(await chip("degraded", {}), "Vendor");
  assert.equal(await chip("loaded", {}), "Vendor");
});

test("catalogue-free runtime stays neutral while model controls are unavailable", async () => {
  const provider = {...descriptor("unsupported"), available_models:[], agent_runtime:{}} as ProviderDescriptor;
  for (const controls of [undefined, {model:"launch-only"}]) {
    const view = await mount(provider, undefined, undefined, undefined, controls);
    try {
      assert.equal(view.trigger(), null);
      assert.equal(view.host.textContent, `${controls?.model ?? "deep"} · high`);
      assert.doesNotMatch(view.host.textContent!, /Default/);
      assert.equal(view.picker(), null);
    } finally { await view.unmount(); }
  }
});

test("an empty runtime catalogue preserves the tab model and exposes catalogue refresh", async () => {
  const provider = { ...descriptor("live"), available_models: [], transport: "chat_backend", agent_runtime: {
    runtimeModes: [{ id: "full-access", displayName: "Full access" }],
  } } as ProviderDescriptor;
  const view = await mount(provider);
  try {
    assert.equal(view.trigger()!.textContent, "deep · high");
    await act(async () => view.trigger()!.click());
    assert.ok(view.option("deep"));
    assert.ok(document.body.querySelector('[aria-label="Refresh models"]'));
    const advanced = document.body.querySelector("details")!;
    assert.equal(advanced.open, false);
    assert.equal(advanced.querySelector("summary")?.textContent, "Advanced");
    assert.equal(button("Apply").disabled, true);
  } finally { await view.unmount(); }
});
