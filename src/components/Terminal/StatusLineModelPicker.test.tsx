import "./ChatComposer.composition.harness";
import assert from "node:assert/strict";
import { test } from "node:test";
import { act } from "react";
import { createRoot } from "react-dom/client";
import type { ProviderDescriptor } from "../../lib/types";
import { __setModelCatalogueRefreshApiForTest } from "../../lib/modelCatalogueRefresh";
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

async function mount(provider: ProviderDescriptor | undefined) {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  toasts.length = 0;
  const render = (pendingSelection?: { model: string; effort: string | null }) => root.render(
    <MultiplexerActionsContext.Provider value={toastActions}>
    <StatusLineModelPicker
      tabId="tab"
      descriptor={provider}
      facts={{ hasSession: true }}
      label="Vendor · deep · high"
      modelCandidates={["deep"]}
      currentEffort="high"
      pendingSelection={pendingSelection}
    />
    </MultiplexerActionsContext.Provider>,
  );
  await act(async () => render());
  return {
    host,
    async setPendingSelection(selection?: { model: string; effort: string | null }) {
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
    assert.deepEqual(fallback, [["tab", "fast", undefined]]);
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
  __setModelCatalogueRefreshApiForTest({ refresh: async (id) => { refreshed.push(id); return true; } });
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
