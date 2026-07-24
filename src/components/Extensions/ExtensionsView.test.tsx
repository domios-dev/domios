// Run: npx tsx src/components/Extensions/ExtensionsView.test.tsx

import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { ExtensionsView } from "./ExtensionsView";
import type { PluginSnapshot } from "./types";
import { __setInvokeForTest } from "../../lib/bridge";
import { __resetPluginSnapshotStoreForTest } from "../../lib/pluginSnapshotStore";

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
  const window = new Window({ url: "http://localhost/" });
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
  Object.defineProperty(globalThis, "HTMLButtonElement", { value: window.HTMLButtonElement, configurable: true });
  Object.defineProperty(globalThis, "SVGElement", { value: window.SVGElement, configurable: true });
  Object.defineProperty(globalThis, "Element", { value: window.Element, configurable: true });
  Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
  Object.defineProperty(globalThis, "Event", { value: window.Event, configurable: true });
  Object.defineProperty(globalThis, "CustomEvent", { value: window.CustomEvent, configurable: true });
  Object.defineProperty(globalThis, "FormData", { value: window.FormData, configurable: true });
  Object.defineProperty(globalThis, "setTimeout", { value: window.setTimeout.bind(window), configurable: true });
}

async function render(ui: React.ReactElement): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(ui);
  });
  return { host, root };
}

async function flush() {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

const SNAPSHOT: PluginSnapshot = {
  plugins: [],
  channels: [
    {
      name: "codeterm-plugins",
      url: "https://github.com/rollacode/codeterm-plugins.git",
      path: "/cache/codeterm-plugins",
      kind: "git",
      system: true,
      removable: false,
      verified: true,
      refreshed_at: "2026-06-19T08:00:00.000Z",
    },
    {
      name: "lab",
      url: "https://example.com/lab-channel.git",
      path: "/cache/lab",
      kind: "git",
      removable: true,
    },
  ],
  diff: [],
  marketplace: [
    { channel: "codeterm-plugins", id: "git", drift: "same", remote_version: "1.0.0" },
    { channel: "codeterm-plugins", id: "github", drift: "same", remote_version: "1.0.0" },
    { channel: "lab", id: "scratchpad", drift: "same", remote_version: "0.1.0" },
  ],
};

const SNAPSHOT_WITH_NEW: PluginSnapshot = {
  ...SNAPSHOT,
  channels: [
    ...SNAPSHOT.channels,
    {
      name: "tools",
      url: "https://example.com/tools.git",
      path: "/cache/tools",
      kind: "git",
      removable: true,
    },
  ],
};

function installInvokeMock(snapshot: PluginSnapshot = SNAPSHOT) {
  const calls: Array<{ cmd: string; args?: Record<string, unknown> }> = [];
  let current = snapshot;
  __setInvokeForTest(async <T,>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
    calls.push({ cmd, args });
    if (cmd === "get_config") return { plugins: {} } as T;
    if (cmd === "plugin_manager_snapshot") return current as T;
    if (cmd === "plugin_manager_action") {
      const action = args?.action as { kind?: string; name?: string } | undefined;
      if (action?.kind === "add-channel") current = SNAPSHOT_WITH_NEW;
      return current as T;
    }
    throw new Error(`unexpected invoke: ${cmd}`);
  });
  return calls;
}

async function renderExtensions() {
  __resetPluginSnapshotStoreForTest();
  const calls = installInvokeMock();
  const view = await render(
    <ExtensionsView
      visible
      pluginActivities={new Map()}
      pluginActivityDisplayMap={new Map()}
    />,
  );
  await flush();
  return { ...view, calls };
}

function button(host: HTMLElement, selector: string): HTMLButtonElement {
  const node = host.querySelector(selector);
  assert(node instanceof window.HTMLButtonElement, `${selector} should be a button`);
  return node;
}

function changeInput(input: HTMLInputElement, value: string) {
  const setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, "value")?.set;
  setter?.call(input, value);
  input.dispatchEvent(new window.Event("input", { bubbles: true }));
  input.dispatchEvent(new window.Event("change", { bubbles: true }));
}

async function openChannels(host: HTMLElement) {
  await act(async () => {
    button(host, '[data-extension-filter="channels"]').click();
  });
  await flush();
}

function cleanup() {
  __resetPluginSnapshotStoreForTest();
  __setInvokeForTest(null);
}

setupDom();

test("selecting a plugin opens its detail view and back returns to the list", async () => {
  const { host, root } = await renderExtensions();
  const row = host.querySelector('[data-testid="extensions-plugin-row"]');
  assert(row instanceof window.HTMLElement, "plugin row should render");

  await act(async () => {
    row.click();
  });

  const layout = host.querySelector("[data-narrow-view]");
  assert(layout?.getAttribute("data-narrow-view") === "detail", "selection should open the narrow detail view");
  assert(host.querySelector('[data-testid="extensions-detail-back"]'), "detail view should provide a back control");

  await act(async () => {
    button(host, '[data-testid="extensions-detail-back"]').click();
  });
  assert(layout?.getAttribute("data-narrow-view") === "list", "back should restore the narrow plugin list");

  await act(async () => root.unmount());
  cleanup();
});

test("Channels tab renders compact collapsed channel rows", async () => {
  const { host, root } = await renderExtensions();
  await openChannels(host);

  const rows = Array.from(host.querySelectorAll('[data-testid="extensions-channel-row"]'));
  assert(rows.length === 2, `expected 2 channel rows, got ${rows.length}`);
  assert(rows[0].textContent?.includes("codeterm-plugins"), "official channel row should render");
  assert(rows[0].textContent?.includes("2 plugins"), "channel row should show plugin count");
  assert(!rows[0].textContent?.includes("github.com"), "channel row should stay collapsed without URL detail");

  await act(async () => root.unmount());
  cleanup();
});

test("selecting a channel shows channel attributes in the preview", async () => {
  const { host, root } = await renderExtensions();
  await openChannels(host);

  await act(async () => {
    button(host, '[data-channel-name="lab"]').click();
  });

  const detail = host.querySelector('[data-testid="extensions-channel-detail"]');
  assert(detail?.textContent?.includes("lab"), "preview should show selected channel name");
  assert(detail?.textContent?.includes("https://example.com/lab-channel.git"), "preview should show selected channel URL");
  assert(detail?.textContent?.includes("1"), "preview should show selected channel plugin count");
  assert(detail?.textContent?.includes("custom"), "preview should show custom source");

  await act(async () => root.unmount());
  cleanup();
});

test("Add channel toggles the inline form", async () => {
  const { host, root } = await renderExtensions();
  await openChannels(host);

  assert(!host.querySelector('[data-testid="extensions-channel-add-form"]'), "add form should start closed");
  await act(async () => {
    button(host, '[data-testid="extensions-channel-add-open"]').click();
  });
  assert(host.querySelector('[data-testid="extensions-channel-add-form"]'), "add form should open inline");

  await act(async () => root.unmount());
  cleanup();
});

test("opening Extensions refreshes a snapshot changed outside the view", async () => {
  __resetPluginSnapshotStoreForTest();
  const calls = installInvokeMock();
  const { root } = await render(
    <ExtensionsView visible pluginActivities={new Map()} pluginActivityDisplayMap={new Map()} />,
  );
  await flush();

  const snapshotCalls = () => calls.filter((c) => c.cmd === "plugin_manager_snapshot");
  assert(snapshotCalls().length === 1, `expected 1 plugin_manager_snapshot on first open, got ${snapshotCalls().length}`);

  await act(async () => {
    root.render(<ExtensionsView visible={false} pluginActivities={new Map()} pluginActivityDisplayMap={new Map()} />);
  });
  await flush();
  const before = calls.length;
  await act(async () => {
    root.render(<ExtensionsView visible pluginActivities={new Map()} pluginActivityDisplayMap={new Map()} />);
  });
  await flush();

  const newSnapshots = calls.slice(before).filter((c) => c.cmd === "plugin_manager_snapshot");
  assert(newSnapshots.length === 1, `expected 1 plugin_manager_snapshot call on re-open, got ${newSnapshots.length}`);

  await act(async () => root.unmount());
  cleanup();
});

test("submitting Add channel calls add-channel then refresh-channel and renders cached snapshot", async () => {
  const { host, root, calls } = await renderExtensions();
  await openChannels(host);
  await act(async () => {
    button(host, '[data-testid="extensions-channel-add-open"]').click();
  });

  const name = host.querySelector('[data-testid="extensions-channel-name-input"]');
  const url = host.querySelector('[data-testid="extensions-channel-url-input"]');
  assert(name instanceof window.HTMLInputElement, "name input should render");
  assert(url instanceof window.HTMLInputElement, "url input should render");

  await act(async () => {
    changeInput(name, "tools");
    changeInput(url, "https://example.com/tools.git");
  });
  await flush();
  await act(async () => {
    button(host, '[data-testid="extensions-channel-add-submit"]').click();
  });
  await flush();

  const actionCalls = calls.filter((call) => call.cmd === "plugin_manager_action");
  const addCall = actionCalls.find((call) => {
    const action = call.args?.action as { kind?: string; name?: string; url?: string } | undefined;
    return action?.kind === "add-channel";
  });
  assert(addCall, "submit should invoke plugin_manager_action add-channel");
  const action = addCall.args?.action as { name?: string; url?: string };
  assert(action.name === "tools", `expected name tools, got ${action.name}`);
  assert(action.url === "https://example.com/tools.git", `expected tools URL, got ${action.url}`);
  const refreshIndex = actionCalls.findIndex((call) => {
    const nextAction = call.args?.action as { kind?: string; channel?: string } | undefined;
    return nextAction?.kind === "refresh-channel" && nextAction.channel === "tools";
  });
  const addIndex = actionCalls.indexOf(addCall);
  assert(refreshIndex > addIndex, "refresh-channel for the new channel should occur after add-channel");
  assert(host.querySelector('[data-channel-name="tools"]'), "new channel should render after add");
  assert(!host.querySelector('[data-testid="plugin-error-toast"]'), "extensions flow should not route through global/plugin toast state");

  await act(async () => root.unmount());
  cleanup();
});

test("first loading state renders before snapshot resolves", async () => {
  __resetPluginSnapshotStoreForTest();
  let release!: () => void;
  __setInvokeForTest(async <T,>(cmd: string): Promise<T> => {
    if (cmd === "get_config") return { plugins: {} } as T;
    if (cmd === "plugin_manager_snapshot") {
      await new Promise<void>((resolve) => {
        release = resolve;
      });
      return SNAPSHOT as T;
    }
    throw new Error(`unexpected invoke: ${cmd}`);
  });

  const { host, root } = await render(
    <ExtensionsView visible pluginActivities={new Map()} pluginActivityDisplayMap={new Map()} />,
  );
  assert(host.querySelector('[data-testid="extensions-loading"]'), "loading state should render before first snapshot resolves");

  await act(async () => {
    release();
  });
  await flush();

  assert(!host.querySelector('[data-testid="extensions-loading"]'), "loading state should clear after snapshot resolves");
  await act(async () => root.unmount());
  cleanup();
});

test("refresh failure preserves cached rows and shows error", async () => {
  __resetPluginSnapshotStoreForTest();
  let snapshotCalls = 0;
  __setInvokeForTest(async <T,>(cmd: string): Promise<T> => {
    if (cmd === "get_config") return { plugins: {} } as T;
    if (cmd === "plugin_manager_snapshot") {
      snapshotCalls += 1;
      if (snapshotCalls === 1) return SNAPSHOT as T;
      throw new Error("snapshot refresh failed");
    }
    if (cmd === "plugin_manager_action") {
      throw new Error("snapshot refresh failed");
    }
    throw new Error(`unexpected invoke: ${cmd}`);
  });

  const { host, root } = await render(
    <ExtensionsView visible pluginActivities={new Map()} pluginActivityDisplayMap={new Map()} />,
  );
  await flush();

  await act(async () => {
    button(host, '[data-testid="extensions-refresh"]').click();
  });
  await flush();

  assert(host.querySelector('[data-testid="extensions-error"]'), "refresh failure should show extensions-error");
  assert(!host.querySelector('[data-testid="extensions-loading"]'), "refresh failure with cached rows should not fall back to first-load state");
  assert(host.querySelector('[data-channel-name="lab"]'), "cached rows should remain rendered after refresh failure");
  assert(!host.querySelector('[data-testid="plugin-error-toast"]'), "refresh failure should stay inline and not use toast state");

  await act(async () => root.unmount());
  cleanup();
});

async function main() {
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
  console.log(`ExtensionsView tests: ${tests.length - failed}/${tests.length} passed in ${elapsedMs.toFixed(1)}ms`);
  if (failed > 0) throw new Error(`${failed} test(s) failed`);
}

void main();
