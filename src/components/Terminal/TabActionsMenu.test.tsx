// Run: npx tsx src/components/Terminal/TabActionsMenu.test.tsx

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { TabActionsMenu } from "./TabActionsMenu";
import { buildTabMenu, type TabMenuAction, type TabMenuInput } from "./tabMenuModel";
import { setTabPermissionAutoConfirm } from "./tabSessionActions";
import { closeTelegramThread, fetchTelegramThreadState, openTelegramThread } from "./telegramThreadActions";
import { __setInvokeForTest } from "../../lib/bridge";
import type { ProviderDescriptor, Tab } from "../../lib/types";

type TestFn = () => void | Promise<void>;

const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/", width: 1280, height: 800 });
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  for (const key of ["document", "navigator"] as const) {
    Object.defineProperty(globalThis, key, { value: window[key], configurable: true });
  }
  for (const key of [
    "HTMLElement",
    "HTMLButtonElement",
    "SVGElement",
    "Element",
    "Node",
    "Event",
    "CustomEvent",
    "MouseEvent",
    "KeyboardEvent",
  ] as const) {
    Object.defineProperty(globalThis, key, { value: window[key], configurable: true });
  }
  Object.defineProperty(globalThis, "requestAnimationFrame", {
    value: (callback: (time: number) => void) => {
      callback(0);
      return 0;
    },
    configurable: true,
  });
  Object.defineProperty(globalThis, "cancelAnimationFrame", { value: () => {}, configurable: true });
}

const DESCRIPTOR = {
  schema_version: 1,
  id: "fixture-provider",
  display_name: "fixture-provider",
  transport: "pty",
  system_prompt_delivery: "external",
  available_models: [],
  settings_schema: [],
  notifications_schema: [],
  supports_lazy_skills: false,
  has_hooks: false,
  supports_permission_auto_confirm: true,
} as unknown as ProviderDescriptor;

const TAB: Tab = {
  id: "tab-1",
  pty_id: "pty-1",
  tab_type: "Terminal",
  title: "Worker",
  cwd: null,
  agent_type: DESCRIPTOR.id,
  agent_session_id: "session-1",
};

function headerInput(overrides: Partial<TabMenuInput> = {}): TabMenuInput {
  return {
    tab: TAB,
    surface: "header",
    status: null,
    descriptor: DESCRIPTOR,
    descriptors: [DESCRIPTOR],
    activity: null,
    orchestrators: undefined,
    canRename: false,
    canClose: false,
    pluginTabMenu: {
      items: [{
        pluginId: "ring",
        itemId: "receiver",
        label: "fixture-plugin-label",
        action: "receiverTarget",
        iconSvg: '<svg viewBox="0 0 1 1"><path d="M0 0"/></svg>',
        iconColor: "#336699",
      }],
      receiverTargets: { ring: TAB.id },
    },
    telegramThread: { bridgeActive: true, topics: "available", bound: true, link: null },
    ...overrides,
  };
}

interface Mounted {
  host: HTMLDivElement;
  root: Root;
  actions: TabMenuAction[];
  render(input: TabMenuInput): Promise<void>;
}

async function mountOpen(input: TabMenuInput): Promise<Mounted> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  const actions: TabMenuAction[] = [];
  const render = async (next: TabMenuInput) => {
    await act(async () => {
      root.render(<TabActionsMenu entries={buildTabMenu(next)} busy={false} onAction={(action) => actions.push(action)} />);
    });
  };
  await render(input);
  const trigger = byTestId("tab-actions-trigger");
  assert(trigger, "menu trigger rendered");
  trigger.focus();
  await act(async () => {
    trigger.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }));
  });
  assert(byTestId("tab-actions-menu")?.getAttribute("role") === "menu", "menu opened from the keyboard");
  assert(trigger.getAttribute("aria-expanded") === "true", "trigger reports the open menu");
  return { host, root, actions, render };
}

function byTestId(testId: string): HTMLButtonElement | null {
  return document.querySelector<HTMLButtonElement>(`[data-testid="${testId}"]`);
}

function focusedTestId(): string | null {
  return (document.activeElement as HTMLElement | null)?.getAttribute("data-testid") ?? null;
}

async function key(target: Element, name: string) {
  await act(async () => {
    target.dispatchEvent(new KeyboardEvent("keydown", { key: name, bubbles: true, cancelable: true }));
  });
}

function describedText(element: Element | null): string {
  const id = element?.getAttribute("aria-describedby");
  return id ? (document.getElementById(id)?.textContent ?? "") : "";
}

async function cleanup({ host, root }: Mounted) {
  await act(async () => root.unmount());
  host.remove();
}

test("the auto-confirm checkbox reports its state, requests the flip and keeps the menu open", async () => {
  const mounted = await mountOpen(headerInput({ pluginTabMenu: { items: [], receiverTargets: {} } }));
  const item = byTestId("tab-permission-auto-confirm");
  assert(item, "auto-confirm item is rendered");
  assert(item.closest('[role="menu"]'), "auto-confirm lives in the tab actions menu");
  assert(item.getAttribute("role") === "menuitemcheckbox", "auto-confirm is a menuitemcheckbox");
  assert(item.getAttribute("aria-checked") === "false", "auto-confirm reports the persisted opt-out");
  assert(describedText(item).length > 0, "auto-confirm is described");
  await act(async () => item.click());
  assert(JSON.stringify(mounted.actions) === JSON.stringify([{ type: "setAutoConfirm", enabled: true }]), "toggle requests enabling");
  assert(byTestId("tab-permission-auto-confirm"), "toggling keeps the menu open");

  await mounted.render(headerInput({ tab: { ...TAB, permission_auto_confirm: true } }));
  const enabled = byTestId("tab-permission-auto-confirm");
  assert(enabled?.getAttribute("aria-checked") === "true", "auto-confirm reflects the persisted opt-in");
  await act(async () => enabled.click());
  assert(JSON.stringify(mounted.actions[1]) === JSON.stringify({ type: "setAutoConfirm", enabled: false }), "an enabled toggle requests disabling");
  await cleanup(mounted);
});

test("the Plugins submenu is entered and left from the keyboard with focus kept in the menu", async () => {
  const mounted = await mountOpen(headerInput());
  const plugins = byTestId("tab-menu-plugins");
  assert(plugins?.getAttribute("aria-haspopup") === "menu", "Plugins announces a submenu");
  assert(!byTestId("tab-menu-plugin-ring-receiver"), "plugin items are not listed at the top level");
  assert(!byTestId("tab-menu-telegram-connect"), "Telegram is not listed at the top level");

  plugins.focus();
  await key(plugins, "ArrowRight");
  assert(focusedTestId() === "tab-menu-plugin-ring-receiver", `entering focuses the first submenu item, got ${focusedTestId()}`);
  const plugin = byTestId("tab-menu-plugin-ring-receiver");
  assert(plugin?.getAttribute("role") === "menuitemcheckbox", "plugin item is a checkbox");
  assert(plugin.getAttribute("aria-checked") === "true", "bound plugin item is checked");
  assert(plugin.querySelector("svg")?.getAttribute("aria-hidden") === "true", "plugin icon renders decoratively");
  assert(byTestId("tab-menu-telegram-connect")?.getAttribute("role") === "menuitem", "Attach Telegram is inside Plugins");
  assert(byTestId("tab-menu-telegram-disconnect")?.getAttribute("role") === "menuitem", "Detach Telegram is inside Plugins");
  assert(!byTestId("chat-reconnect"), "the submenu replaces the root actions");

  await key(plugin, "ArrowLeft");
  assert(focusedTestId() === "tab-menu-plugins", `leaving returns focus to Plugins, got ${focusedTestId()}`);

  await act(async () => byTestId("tab-menu-plugins")?.click());
  assert(byTestId("tab-menu-telegram-connect"), "activating Plugins opens the submenu");
  await key(document.activeElement as Element, "Escape");
  assert(focusedTestId() === "tab-menu-plugins", "Escape inside the submenu returns to Plugins");
  assert(byTestId("tab-actions-menu"), "Escape inside the submenu keeps the menu open");
  await key(document.activeElement as Element, "Escape");
  assert(!byTestId("tab-actions-menu"), "Escape at the root closes the menu");
  assert(focusedTestId() === "tab-actions-trigger", "closing returns focus to the trigger");
  await cleanup(mounted);
});

test("a submenu action reports its typed action once and closes the menu", async () => {
  const mounted = await mountOpen(headerInput());
  await act(async () => byTestId("tab-menu-plugins")?.click());
  await act(async () => byTestId("tab-menu-telegram-disconnect")?.click());
  assert(JSON.stringify(mounted.actions) === JSON.stringify([{ type: "closeTelegram" }]), "Detach reports closeTelegram");
  assert(!byTestId("tab-actions-menu"), "the menu closes after the action");
  await cleanup(mounted);
});

test("an unavailable Attach stays focusable, explains itself and does nothing", async () => {
  const mounted = await mountOpen(headerInput({
    telegramThread: { bridgeActive: false, topics: "unknown", bound: false, link: null },
  }));
  await act(async () => byTestId("tab-menu-plugins")?.click());
  const attach = byTestId("tab-menu-telegram-connect");
  assert(attach && !attach.disabled, "Attach stays focusable");
  assert(attach.getAttribute("aria-disabled") === "true", "Attach is announced as unavailable");
  assert(describedText(attach).length > 0, "Attach carries its reason");
  assert(!byTestId("tab-menu-telegram-disconnect"), "an unbound tab offers no Detach");
  await act(async () => attach.click());
  assert(mounted.actions.length === 0, "an unavailable Attach runs nothing");
  assert(byTestId("tab-actions-menu"), "an unavailable Attach keeps the menu open");
  await cleanup(mounted);
});

test("auto-confirm and Telegram go through their bridge commands", async () => {
  const calls: Array<[string, unknown]> = [];
  __setInvokeForTest((async (command: string, args?: unknown) => {
    calls.push([command, args]);
    if (command === "bridge_thread_state") return { bridge_active: true, topics: "unavailable", bound: false, link: null };
    if (command === "bridge_thread_open") return { link: "https://t.me/example_bot/7" };
    return true;
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
  }) as any);
  try {
    await setTabPermissionAutoConfirm("tab-7", true);
    const state = await fetchTelegramThreadState("tab-7");
    assert(JSON.stringify(state) === JSON.stringify({ bridgeActive: true, topics: "unavailable", bound: false, link: null }), "thread state maps from the wire");
    assert(await openTelegramThread("tab-7") === "https://t.me/example_bot/7", "open returns the topic link");
    await closeTelegramThread("tab-7");
  } finally {
    __setInvokeForTest(null);
  }
  assert(JSON.stringify(calls) === JSON.stringify([
    ["mux_set_permission_auto_confirm", { tabId: "tab-7", enabled: true }],
    ["bridge_thread_state", { tabId: "tab-7" }],
    ["bridge_thread_open", { tabId: "tab-7" }],
    ["bridge_thread_close", { tabId: "tab-7" }],
  ]), `commands carry the tab through: ${JSON.stringify(calls)}`);
});

async function main() {
  setupDom();
  const started = performance.now();
  let failed = 0;
  for (const [name, fn] of tests) {
    try {
      await fn();
      console.log(`✓ ${name}`);
    } catch (error) {
      failed += 1;
      console.error(`✗ ${name}`);
      console.error(error);
    }
  }
  const elapsedMs = performance.now() - started;
  console.log(`TabActionsMenu tests: ${tests.length - failed}/${tests.length} passed in ${elapsedMs.toFixed(1)}ms`);
  if (failed > 0) throw new Error(`${failed} test(s) failed`);
}

void main();
