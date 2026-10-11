// Run: npx tsx src/components/Extensions/ExtensionsView.test.tsx

import React, { act } from "react";
import type { Root } from "react-dom/client";
import { Window } from "happy-dom";
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
  // React DOM detects native input-event support when its client entrypoint loads.
  const { createRoot } = await import("react-dom/client");
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(ui);
  });
  return { host, root };
}

async function renderExtensionsView(): Promise<{ host: HTMLDivElement; root: Root }> {
  const { ExtensionsView } = await import("./ExtensionsView");
  return render(
    <ExtensionsView visible pluginActivities={new Map()} pluginActivityDisplayMap={new Map()} />,
  );
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

const UPDATE_SNAPSHOT: PluginSnapshot = {
  plugins: [
    { id: "git", display_name: "Git", version: "0.1.4", removable: true, capabilities: { view: true }, source: { kind: "channel", channel: "codeterm-plugins", path: "git" }, update: { available: true, reason: "newer_version", remote_version: "0.1.5" } },
  ],
  channels: SNAPSHOT.channels,
  diff: [],
  marketplace: [
    { channel: "codeterm-plugins", id: "git", installed: true, local_version: "0.1.4", remote_version: "0.1.5", drift: "update_available" },
  ],
};

const UPDATED_SNAPSHOT: PluginSnapshot = {
  ...UPDATE_SNAPSHOT,
  plugins: UPDATE_SNAPSHOT.plugins.map((plugin) => ({ ...plugin, version: "0.1.5", update: { available: false, reason: "same_version", remote_version: "0.1.5" } })),
  marketplace: [
    { channel: "codeterm-plugins", id: "git", installed: true, local_version: "0.1.5", remote_version: "0.1.5", drift: "up_to_date" },
  ],
};

const NETWORK_SNAPSHOT: PluginSnapshot = {
  plugins: [{
    id: "lmstudio",
    display_name: "LM Studio",
    removable: true,
    network_allow: ["old.example.test"],
    effective_network_allow: ["old.example.test"],
  }],
  channels: SNAPSHOT.channels,
  diff: [],
  marketplace: [{ channel: "codeterm-plugins", id: "lmstudio", installed: true, drift: "same" }],
};

function installInvokeMock(snapshot: PluginSnapshot = SNAPSHOT, config: unknown = { plugins: {} }) {
  const calls: Array<{ cmd: string; args?: Record<string, unknown> }> = [];
  let current = structuredClone(snapshot);
  __setInvokeForTest(async <T,>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
    calls.push({ cmd, args });
    if (cmd === "get_config") return config as T;
    if (cmd === "plugin_manager_snapshot") return current as T;
    if (cmd === "plugin_manager_action") {
      const action = args?.action as { kind?: string; name?: string } | undefined;
      if (action?.kind === "add-channel") current = SNAPSHOT_WITH_NEW;
      if (action?.kind === "settings") {
        const setting = args?.action as { pluginId: string; allowHosts?: string[]; disallowHosts?: string[] };
        const plugin = current.plugins.find((entry) => entry.id === setting.pluginId);
        if (plugin) {
          const allowed = [...(plugin.network_allow ?? [])];
          for (const host of setting.allowHosts ?? []) if (!allowed.includes(host)) allowed.push(host);
          plugin.network_allow = allowed.filter((host) => !(setting.disallowHosts ?? []).includes(host));
          plugin.effective_network_allow = plugin.network_allow;
        }
      }
      return current as T;
    }
    throw new Error(`unexpected invoke: ${cmd}`);
  });
  return calls;
}

async function renderExtensions(snapshot: PluginSnapshot = SNAPSHOT) {
  __resetPluginSnapshotStoreForTest();
  const calls = installInvokeMock(snapshot);
  const view = await renderExtensionsView();
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

test("installed plugin rows have no titlebar pin control", async () => {
  const { host, root } = await renderExtensions(UPDATE_SNAPSHOT);
  const row = host.querySelector('[data-testid="extensions-plugin-row"][data-plugin-id="git"]');
  assert(row, "installed plugin row should render");
  assert(row.querySelectorAll("button").length === 0, "installed plugin rows should not render a secondary control");

  await act(async () => root.unmount());
  cleanup();
});

test("selecting a plugin opens its detail view and back returns to the list", async () => {
  const { host, root } = await renderExtensions();
  const row = host.querySelector('[data-testid="extensions-plugin-row"]');
  assert(row instanceof window.HTMLElement, "plugin row should render");

  await act(async () => {
    row.click();
  });

  const layout = host.querySelector("[data-narrow-view]");
  assert(layout?.getAttribute("data-narrow-view") === "detail", "selection should open the narrow detail view");
  assert(host.querySelector('button[aria-label="Back to Plugins"]'), "detail view provides a named return control");

  await act(async () => {
    button(host, '[data-testid="extensions-detail-back"]').click();
  });
  assert(layout?.getAttribute("data-narrow-view") === "list", "back should restore the narrow plugin list");

  await act(async () => root.unmount());
  cleanup();
});

test("Allowed hosts editor adds and removes grants through plugin settings action", async () => {
  __resetPluginSnapshotStoreForTest();
  const calls = installInvokeMock(NETWORK_SNAPSHOT, {
    plugins: { lmstudio: { baseUrl: "http://localai.example.test:1234" } },
  });
  const { host, root } = await renderExtensionsView();
  await flush();
  await act(async () => {
    host.querySelector<HTMLElement>('[data-testid="extensions-plugin-row"][data-plugin-id="lmstudio"]')?.click();
  });
  assert(host.querySelector('[data-testid="extensions-allowed-hosts"]'), "allowed hosts editor should render");
  const inlineAllow = host.querySelector<HTMLButtonElement>('[aria-label="Allow localai.example.test:1234"]');
  assert(inlineAllow, "configured base URL host should have an inline allow action");
  await act(async () => inlineAllow.click());
  await flush();

  const hostInput = host.querySelector<HTMLInputElement>('[aria-label="Host or host:port"]');
  assert(hostInput, "host entry control should have an accessible name");
  await act(async () => changeInput(hostInput, "new.example.test:9443"));
  const allowHost = button(host, '[aria-label="Allow host"]');
  assert(!allowHost.disabled, "typing a host should enable the allow control");
  await act(async () => allowHost.click());
  await flush();

  const actionCalls = calls.filter((call) => call.cmd === "plugin_manager_action");
  const add = actionCalls.find((call) => {
    const action = call.args?.action as { kind?: string; allowHosts?: string[] } | undefined;
    return action?.kind === "settings" && action.allowHosts?.includes("new.example.test:9443");
  });
  assert(add, "adding a host should call the shared settings route");
  const remove = host.querySelector<HTMLButtonElement>('[aria-label="Remove old.example.test"]');
  assert(remove, "the current user-granted host should have a named remove button");
  await act(async () => remove.click());
  await flush();
  const removeCall = calls.filter((call) => call.cmd === "plugin_manager_action").find((call) => {
    const action = call.args?.action as { kind?: string; disallowHosts?: string[] } | undefined;
    return action?.kind === "settings" && action.disallowHosts?.includes("old.example.test");
  });
  assert(removeCall, "removing a host should call the shared settings route");
  assert(!host.textContent?.includes("old.example.test"), "removed host should disappear from the refreshed editor");

  await act(async () => root.unmount());
  cleanup();
});

test("successful update immediately clears stale update state", async () => {
  __resetPluginSnapshotStoreForTest();
  __setInvokeForTest(async <T,>(cmd: string): Promise<T> => {
    if (cmd === "get_config") return { plugins: {} } as T;
    if (cmd === "plugin_manager_snapshot") return UPDATE_SNAPSHOT as T;
    if (cmd === "plugin_manager_action") return UPDATED_SNAPSHOT as T;
    throw new Error(`unexpected invoke: ${cmd}`);
  });
  const { host, root } = await renderExtensionsView();
  await flush();

  await act(async () => {
    host.querySelector<HTMLElement>('[data-testid="extensions-plugin-row"]')?.click();
  });
  assert(host.querySelector('[data-testid="extensions-plugin-update"]'), "newer channel version should offer Update");
  await act(async () => {
    button(host, '[data-testid="extensions-plugin-update"]').click();
  });
  await flush();

  assert(!host.querySelector('[data-testid="extensions-plugin-update"]'), "fresh action snapshot should clear Update");
  assert(!host.textContent?.includes("→ v0.1.4"), "equal post-update versions should not keep an update arrow");

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
  const { ExtensionsView } = await import("./ExtensionsView");
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

  const { host, root } = await renderExtensionsView();
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

  const { ExtensionsView } = await import("./ExtensionsView");
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
