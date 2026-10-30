// Run: npx tsx packages/chat-ui/src/blocks/SettingBlock.test.tsx

import React, { act } from "react";
import { Window } from "happy-dom";
import type { SettingBlockState, SettingControl, SettingInfo, SettingValue } from "./settingBlockModel";

const tests: Array<[string, () => Promise<void>]> = [];
function test(name: string, run: () => Promise<void>) {
  tests.push([name, run]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const testWindow = new Window({ url: "http://localhost/" });
Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { value: true, configurable: true });
for (const key of ["window", "document", "navigator", "HTMLElement", "HTMLInputElement", "HTMLSelectElement", "HTMLButtonElement", "SVGElement", "Element", "Node", "Event"] as const) {
  Object.defineProperty(globalThis, key, { value: key === "window" ? testWindow : testWindow[key], configurable: true });
}
testWindow.matchMedia = (() => ({
  matches: false, media: "", onchange: null,
  addEventListener: () => {}, removeEventListener: () => {},
  addListener: () => {}, removeListener: () => {}, dispatchEvent: () => false,
})) as unknown as typeof testWindow.matchMedia;

function setting(control: SettingControl, value: SettingValue, key = "plugins.chat-preprocessor.enabled"): SettingInfo {
  return { key, label: "Enable preprocessing", description: "When off, messages pass through unchanged.", owner: "Chat Preprocessor", control, value };
}

const UNKNOWN: SettingBlockState = { kind: "refused", reason: "unknown" };

function fakeHost(initial: Record<string, SettingBlockState>) {
  const states = new Map(Object.entries(initial));
  const listeners = new Set<() => void>();
  const calls = { writes: [] as Array<[string, SettingValue]>, reads: [] as string[], opened: [] as string[] };
  const set = (key: string, next: SettingBlockState) => {
    states.set(key, next);
    for (const listener of listeners) listener();
  };
  let fail: string | null = null;
  const host = {
    binding: {
      subscribe(listener: () => void) { listeners.add(listener); return () => { listeners.delete(listener); }; },
      read: (key: string): SettingBlockState => {
        calls.reads.push(key);
        return states.get(key) ?? UNKNOWN;
      },
      refresh: () => {},
    },
    async write(key: string, value: SettingValue) {
      calls.writes.push([key, value]);
      if (fail) throw new Error(fail);
      const current = states.get(key);
      if (current?.kind === "ready") set(key, { kind: "ready", setting: { ...current.setting, value } });
    },
    openSettings: (key: string) => { calls.opened.push(key); },
  };
  return { host, calls, set, failWith: (message: string | null) => { fail = message; } };
}

(async () => {
  const { createRoot } = await import("react-dom/client");
  const { RichMarkdown } = await import("../MarkdownBody");
  const { ChatBlockHostProvider } = await import("./ChatBlockHost");
  type Host = Parameters<typeof ChatBlockHostProvider>[0]["value"];

  async function withRender(text: string, value: Host, run: (element: HTMLDivElement) => Promise<void>) {
    const element = document.createElement("div");
    document.body.appendChild(element);
    const root = createRoot(element);
    try {
      await act(async () => root.render(<ChatBlockHostProvider value={value}><RichMarkdown text={text} /></ChatBlockHostProvider>));
      await run(element);
    } finally {
      await act(async () => root.unmount());
      element.remove();
    }
  }
  const byLabel = (element: HTMLElement, label: string) => element.querySelector(`[aria-label="${label}"]`) as HTMLElement | null;

  test("a_boolean_renders_a_switch_with_schema_label_and_writes_once_per_click", async () => {
    const key = "plugins.chat-preprocessor.enabled";
    const { host, calls, set } = fakeHost({ [key]: { kind: "ready", setting: setting({ kind: "switch" }, false) } });
    await withRender(`@ds-setting ${key}`, { settings: host }, async (element) => {
      assert(element.querySelector("h4")?.textContent === "Enable preprocessing", "schema label missing");
      assert(element.textContent?.includes("When off, messages pass through unchanged."), "schema description missing");
      const toggle = element.querySelector('[role="switch"]') as HTMLButtonElement;
      assert(toggle?.getAttribute("aria-checked") === "false", "switch does not show the value");
      assert(calls.writes.length === 0, "rendering wrote config");
      await act(async () => toggle.click());
      assert(calls.writes.length === 1 && calls.writes[0][0] === key && calls.writes[0][1] === true, `writes ${JSON.stringify(calls.writes)}`);
      assert(element.querySelector('[role="switch"]')?.getAttribute("aria-checked") === "true", "switch did not follow the write");
      await act(async () => set(key, { kind: "ready", setting: setting({ kind: "switch" }, false) }));
      assert(element.querySelector('[role="switch"]')?.getAttribute("aria-checked") === "false", "a change made elsewhere did not show");
      assert(calls.writes.length === 1, "live update wrote config");
    });
  });

  test("enum_number_and_text_controls_write_only_on_user_action", async () => {
    const choice = "plugins.chat-preprocessor.insertPosition";
    const number = "plugins.chat-preprocessor.memK";
    const text = "plugins.demo.greeting";
    const { host, calls } = fakeHost({
      [choice]: { kind: "ready", setting: setting({ kind: "choice", options: [{ value: "prepend", label: "Before message" }, { value: "append", label: "After message" }] }, "prepend", choice) },
      [number]: { kind: "ready", setting: { ...setting({ kind: "stepper", min: 1, max: 8, step: 1 }, 8, number), label: "Memory hits" } },
      [text]: { kind: "ready", setting: { ...setting({ kind: "text" }, "hi", text), label: "Greeting" } },
    });
    await withRender(`@ds-setting ${choice}\n@ds-setting ${number}\n@ds-setting ${text}`, { settings: host }, async (element) => {
      const radios = [...element.querySelectorAll('[role="radio"]')] as HTMLButtonElement[];
      assert(radios.length === 2 && radios[0].getAttribute("aria-checked") === "true", "segmented choice missing");
      await act(async () => radios[1].click());
      assert(calls.writes.length === 1 && calls.writes[0][1] === "append", "choice not written once");

      assert((byLabel(element, "Increase Memory hits") as HTMLButtonElement).disabled, "stepper allows going past max");
      await act(async () => (byLabel(element, "Decrease Memory hits") as HTMLButtonElement).click());
      assert(calls.writes.length === 2 && calls.writes[1][0] === number && calls.writes[1][1] === 7, "stepper not written once");

      const input = element.querySelector('[data-setting-control="text"] input') as HTMLInputElement;
      const save = element.querySelector('[data-setting-control="text"] [data-block-action="save"]') as HTMLButtonElement;
      assert(save.disabled, "Save enabled without a change");
      await act(async () => {
        const setter = Object.getOwnPropertyDescriptor(testWindow.HTMLInputElement.prototype, "value")?.set;
        setter?.call(input, "hello");
        input.dispatchEvent(new Event("input", { bubbles: true }));
        input.dispatchEvent(new Event("change", { bubbles: true }));
      });
      assert(calls.writes.length === 2, "typing wrote config");
      await act(async () => { save.click(); save.click(); });
      assert(calls.writes.length === 3 && calls.writes[2][1] === "hello", `text writes ${JSON.stringify(calls.writes)}`);
    });
  });

  test("a_failed_write_says_so_in_place", async () => {
    const key = "plugins.chat-preprocessor.enabled";
    const { host, failWith } = fakeHost({ [key]: { kind: "ready", setting: setting({ kind: "switch" }, false) } });
    failWith("bounds");
    await withRender(`@ds-setting ${key}`, { settings: host }, async (element) => {
      await act(async () => (element.querySelector('[role="switch"]') as HTMLButtonElement).click());
      assert(element.querySelector('[role="alert"]')?.textContent?.includes("bounds"), "failure not shown");
      assert(element.querySelector('[role="switch"]')?.getAttribute("aria-checked") === "false", "failed write shown as applied");
    });
  });

  test("secret_keys_never_reach_the_host_and_unknown_keys_are_muted", async () => {
    const { host, calls } = fakeHost({});
    await withRender("@ds-setting plugins.telegram-client.bot_token\n@ds-setting plugins.nope.enabled", { settings: host }, async (element) => {
      const notes = [...element.querySelectorAll('[data-chat-block="setting"]')];
      assert(notes.length === 2 && notes.every((note) => note.getAttribute("data-block-missing") === "true"), "refusals not rendered");
      assert(!element.querySelector("button, input, select"), "a refused setting offered a control");
      assert(!calls.reads.includes("plugins.telegram-client.bot_token"), "a secret key was read from the host");
    });
  });

  let failed = 0;
  for (const [name, run] of tests) {
    try {
      await run();
      console.log(`  ✓ ${name}`);
    } catch (error) {
      failed++;
      console.error(`  ✗ ${name}: ${error instanceof Error ? error.message : error}`);
    }
  }
  console.log(`chat-ui/blocks/SettingBlock tests: ${failed === 0 ? "all passed" : `${failed} failed`}`);
  process.exit(failed === 0 ? 0 : 1);
})();
