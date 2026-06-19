// Run: npx tsx src/components/Extensions/ExtensionsView.test.tsx

import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { ExtensionsView } from "./ExtensionsView";
import type { PluginSnapshot } from "./types";
import { __setInvokeForTest } from "../../lib/bridge";

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

setupDom();

test("Channels tab renders compact collapsed channel rows", async () => {
  const { host, root } = await renderExtensions();
  await openChannels(host);

  const rows = Array.from(host.querySelectorAll('[data-testid="extensions-channel-row"]'));
  assert(rows.length === 2, `expected 2 channel rows, got ${rows.length}`);
  assert(rows[0].textContent?.includes("codeterm-plugins"), "official channel row should render");
  assert(rows[0].textContent?.includes("2 plugins"), "channel row should show plugin count");
  assert(!rows[0].textContent?.includes("github.com"), "channel row should stay collapsed without URL detail");

  await act(async () => root.unmount());
  __setInvokeForTest(null);
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
  __setInvokeForTest(null);
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
  __setInvokeForTest(null);
});

test("submitting Add channel calls the add-channel API action", async () => {
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

  const addCall = calls.find((call) => {
    const action = call.args?.action as { kind?: string; name?: string; url?: string } | undefined;
    return call.cmd === "plugin_manager_action" && action?.kind === "add-channel";
  });
  assert(addCall, "submit should invoke plugin_manager_action add-channel");
  const action = addCall.args?.action as { name?: string; url?: string };
  assert(action.name === "tools", `expected name tools, got ${action.name}`);
  assert(action.url === "https://example.com/tools.git", `expected tools URL, got ${action.url}`);
  assert(host.querySelector('[data-channel-name="tools"]'), "new channel should render after add");

  await act(async () => root.unmount());
  __setInvokeForTest(null);
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
