// Run: npx tsx src/components/shared/NumericInput.test.tsx

import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { describe, expect, finish, it } from "../../test/standaloneTest";
import { NumericInput } from "./NumericInput";
import { AgentsOverview } from "../Settings/AgentsTab";
import { defaultConfig } from "../../hooks/useConfig";
import { __setInvokeForTest, __setListenForTest } from "../../lib/bridge";

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const win = new Window({ url: "http://localhost/" });
  for (const key of [
    "window",
    "document",
    "navigator",
    "HTMLElement",
    "HTMLInputElement",
    "HTMLButtonElement",
    "HTMLLabelElement",
    "SVGElement",
    "Element",
    "Node",
    "Event",
    "KeyboardEvent",
    "FocusEvent",
  ] as const) {
    Object.defineProperty(globalThis, key, {
      value: (win as never as Record<string, unknown>)[key],
      configurable: true,
    });
  }
}

function accessibleNameOf(el: HTMLInputElement): string {
  const ariaLabel = el.getAttribute("aria-label")?.trim();
  if (ariaLabel) return ariaLabel;
  const labelledBy = el.getAttribute("aria-labelledby")?.trim();
  if (labelledBy) {
    return labelledBy
      .split(/\s+/)
      .map((id) => document.getElementById(id)?.textContent?.trim() ?? "")
      .filter(Boolean)
      .join(" ");
  }
  if (el.id) {
    const label = document.querySelector(`label[for="${el.id}"]`);
    if (label?.textContent?.trim()) return label.textContent.trim();
  }
  const wrappingLabel = el.closest("label");
  if (wrappingLabel) {
    const clone = wrappingLabel.cloneNode(true) as HTMLElement;
    for (const control of Array.from(clone.querySelectorAll("input, select, textarea"))) {
      control.remove();
    }
    const text = clone.textContent?.trim();
    if (text) return text;
  }
  return "";
}

function setInputValue(input: HTMLInputElement, value: string) {
  const setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, "value")?.set;
  setter?.call(input, value);
  input.dispatchEvent(new Event("input", { bubbles: true }));
  input.dispatchEvent(new Event("change", { bubbles: true }));
}

function pressArrow(input: HTMLInputElement, key: "ArrowUp" | "ArrowDown") {
  input.dispatchEvent(
    new window.KeyboardEvent("keydown", { key, code: key, bubbles: true, cancelable: true }) as unknown as Event,
  );
}

async function mount(ui: React.ReactElement): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(ui);
  });
  return { host, root };
}

setupDom();

describe("NumericInput", () => {
  it("never loses steps on a rapid ArrowDown burst with delayed interleaved echoes", async () => {
    const emitted: number[] = [];
    let externalValue = 0.2;

    const renderInput = (val: number) => (
      <NumericInput
        aria-label="Dedup threshold"
        min={0.05}
        max={0.5}
        step={0.01}
        value={val}
        onValueChange={(next) => emitted.push(next)}
      />
    );

    const { host, root } = await mount(renderInput(externalValue));
    const rerender = async (val: number) => {
      externalValue = val;
      await act(async () => {
        root.render(renderInput(externalValue));
      });
    };

    const input = host.querySelector("input") as HTMLInputElement;
    expect(input.value).toBe("0.2");

    await act(async () => {
      input.focus();
      input.dispatchEvent(new FocusEvent("focus", { bubbles: true }));
    });

    await act(async () => {
      pressArrow(input, "ArrowDown");
      pressArrow(input, "ArrowDown");
    });
    expect(input.value).toBe("0.18");
    expect(emitted).toEqual([0.19, 0.18]);

    // Delayed server echo of the first step arrives mid-burst while focused.
    await rerender(0.19);
    expect(input.value).toBe("0.18");

    await act(async () => {
      pressArrow(input, "ArrowDown");
      pressArrow(input, "ArrowDown");
    });
    expect(input.value).toBe("0.16");

    // Delayed echo of step 2 arrives mid-burst.
    await rerender(0.18);
    expect(input.value).toBe("0.16");

    await act(async () => {
      pressArrow(input, "ArrowDown");
    });
    expect(input.value).toBe("0.15");
    expect(emitted).toEqual([0.19, 0.18, 0.17, 0.16, 0.15]);

    // Remaining delayed echoes arrive.
    await rerender(0.17);
    expect(input.value).toBe("0.15");
    await rerender(0.16);
    expect(input.value).toBe("0.15");
    await rerender(0.15);
    expect(input.value).toBe("0.15");

    await act(async () => root.unmount());
    host.remove();
  });

  it("keeps the final local value on blur while earlier echoes are still in flight", async () => {
    const emitted: number[] = [];
    const renderInput = (val: number) => (
      <NumericInput
        aria-label="Dedup threshold"
        min={0.05}
        max={0.5}
        step={0.01}
        value={val}
        onValueChange={(next) => emitted.push(next)}
      />
    );

    const { host, root } = await mount(renderInput(0.2));
    const rerender = async (val: number) => {
      await act(async () => {
        root.render(renderInput(val));
      });
    };

    const input = host.querySelector("input") as HTMLInputElement;
    await act(async () => {
      input.focus();
      input.dispatchEvent(new FocusEvent("focus", { bubbles: true }));
      for (let i = 0; i < 5; i++) pressArrow(input, "ArrowDown");
    });
    expect(input.value).toBe("0.15");
    expect(emitted).toEqual([0.19, 0.18, 0.17, 0.16, 0.15]);

    // User blurs before any server echo has returned.
    await act(async () => {
      input.blur();
      input.dispatchEvent(new FocusEvent("blur", { bubbles: true }));
    });
    expect(input.value).toBe("0.15");

    // Delayed earlier echoes must not clobber the blurred display.
    await rerender(0.19);
    expect(input.value).toBe("0.15");
    await rerender(0.18);
    expect(input.value).toBe("0.15");

    // Final echo confirms and clears the pending queue.
    await rerender(0.15);
    expect(input.value).toBe("0.15");

    // Subsequent genuinely external updates still apply when blurred.
    await rerender(0.33);
    expect(input.value).toBe("0.33");

    await act(async () => root.unmount());
    host.remove();
  });

  it("re-emits when stepping back to the pre-burst external value before the first echo arrives", async () => {
    const emitted: number[] = [];
    const renderInput = (val: number) => (
      <NumericInput
        aria-label="Dedup threshold"
        min={0.05}
        max={0.5}
        step={0.01}
        value={val}
        onValueChange={(next) => emitted.push(next)}
      />
    );

    const { host, root } = await mount(renderInput(0.2));
    const rerender = async (val: number) => {
      await act(async () => {
        root.render(renderInput(val));
      });
    };

    const input = host.querySelector("input") as HTMLInputElement;
    await act(async () => {
      input.focus();
      input.dispatchEvent(new FocusEvent("focus", { bubbles: true }));
      pressArrow(input, "ArrowDown");
      pressArrow(input, "ArrowUp");
    });

    expect(input.value).toBe("0.2");
    expect(emitted).toEqual([0.19, 0.2]);

    // Echo of the earlier 0.19 write must not overwrite the returned 0.20 draft.
    await rerender(0.19);
    expect(input.value).toBe("0.2");
    await rerender(0.2);
    expect(input.value).toBe("0.2");

    await act(async () => root.unmount());
    host.remove();
  });

  it("reverts invalid drafts on blur and preserves dot-decimal formatting", async () => {
    const emitted: number[] = [];
    const { host, root } = await mount(
      <NumericInput
        aria-label="Dedup threshold"
        min={0.05}
        max={0.5}
        step={0.01}
        value={0.25}
        onValueChange={(next) => emitted.push(next)}
      />,
    );
    const input = host.querySelector("input") as HTMLInputElement;

    await act(async () => {
      input.focus();
      input.dispatchEvent(new FocusEvent("focus", { bubbles: true }));
      setInputValue(input, "0,35");
    });
    expect(emitted).toEqual([]);
    expect(input.getAttribute("aria-invalid")).toBe("true");

    await act(async () => {
      input.blur();
      input.dispatchEvent(new FocusEvent("blur", { bubbles: true }));
    });
    expect(input.value).toBe("0.25");

    await act(async () => root.unmount());
    host.remove();
  });

  it("gives AgentsOverview reminder interval NumericInput an accessible name", async () => {
    __setInvokeForTest(async <T,>(command: string): Promise<T> => {
      if (command === "list_agent_providers") return [] as unknown as T;
      return [] as unknown as T;
    });
    __setListenForTest(async () => () => {});
    const origFetch = globalThis.fetch;
    globalThis.fetch = (async () => ({ ok: true, json: async () => ({}) } as Response)) as typeof fetch;

    const config = defaultConfig();
    const { host, root } = await mount(
      <AgentsOverview
        config={config}
        onUpdate={() => {}}
        onOpenProvider={() => {}}
        onOpenGuide={() => {}}
      />,
    );
    try {
      const reminderInput = host.querySelector<HTMLInputElement>('[data-testid="settings-number-input"]');
      expect(reminderInput !== null).toBe(true);
      expect(accessibleNameOf(reminderInput!)).toBe("Reminder interval (minutes)");
    } finally {
      await act(async () => root.unmount());
      host.remove();
      __setInvokeForTest(null);
      __setListenForTest(null);
      globalThis.fetch = origFetch;
    }
  });
});

finish();
