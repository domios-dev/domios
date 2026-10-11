// Run: node scripts/run-ts-unit-tests.mjs src/components/Extensions/PluginDetailHeader.test.tsx

import assert from "node:assert/strict";
import { act } from "react";
import type { Root } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import { Window } from "happy-dom";
import type { CatalogEntry } from "./pluginCatalog";
import type { ActionKind, ActionOptions } from "./PluginDetailPanel";

const win = new Window({ url: "http://localhost/" });
  Object.assign(win, { __CODETERM_DAEMON__: { origin: /^https?:/.test(win.location.origin) ? win.location.origin : "http://127.0.0.1:7686", token: "test-token" } });
for (const key of ["window", "document", "navigator", "localStorage", "HTMLElement", "HTMLButtonElement", "Element", "Node", "Event", "MouseEvent", "KeyboardEvent", "CustomEvent", "getComputedStyle"]) {
  const value = (win as unknown as Record<string, unknown>)[key];
  Object.defineProperty(globalThis, key, { configurable: true, value: typeof value === "function" && key === "getComputedStyle" ? value.bind(win) : value });
}
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
(globalThis as { requestAnimationFrame: (cb: FrameRequestCallback) => number }).requestAnimationFrame = (cb) => { cb(0); return 0; };
(globalThis as { cancelAnimationFrame: (id: number) => void }).cancelAnimationFrame = () => {};

const telegram: CatalogEntry = {
  id: "telegram-client",
  displayName: "Telegram Client",
  installed: true,
  inMarketplace: true,
  system: false,
  verified: true,
  removable: true,
  dev: false,
  compatible: true,
  updateAvailable: false,
  version: "1.0.0",
  configHelp: "Choose an account.",
};

type Call = { kind: ActionKind; options?: ActionOptions };

async function mount(entry: CatalogEntry, calls: Call[]): Promise<{ host: HTMLDivElement; root: Root }> {
  const { createRoot } = await import("react-dom/client");
  const { PluginDetailHeader } = await import("./PluginDetailHeader");
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () => root.render(
    <PluginDetailHeader entry={entry} busy={null} onAction={(kind, _entry, options) => calls.push({ kind, options })} />,
  ));
  return { host, root };
}

async function unmount({ host, root }: { host: HTMLDivElement; root: Root }) {
  await act(async () => root.unmount());
  host.remove();
  document.body.innerHTML = "";
}

function visibleButtonNames(host: HTMLElement): string[] {
  return [...host.querySelectorAll("button")].map((button) => (button.getAttribute("aria-label") ?? button.textContent ?? "").trim());
}

async function openOverflow(host: HTMLElement): Promise<HTMLElement> {
  const trigger = host.querySelector<HTMLButtonElement>('button[aria-label="More actions for Telegram Client"]');
  assert.ok(trigger, "an installed removable plugin exposes a named overflow menu button");
  assert.equal(trigger.getAttribute("aria-haspopup"), "menu");
  await act(async () => {
    trigger.dispatchEvent(new window.MouseEvent("pointerdown", { bubbles: true, cancelable: true }) as unknown as Event);
  });
  const menu = document.querySelector<HTMLElement>('[role="menu"]');
  assert.ok(menu, "the overflow button opens a menu");
  assert.equal(trigger.getAttribute("aria-expanded"), "true");
  return menu;
}

async function uninstallLivesInOverflowAndStillConfirms() {
  const calls: Call[] = [];
  const view = await mount(telegram, calls);
  try {
    assert.ok(!visibleButtonNames(view.host).some((name) => /uninstall/i.test(name)), "Uninstall is not an always-visible header button");
    const menu = await openOverflow(view.host);
    const item = [...menu.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')].find((node) => /uninstall/i.test(node.textContent ?? ""));
    assert.ok(item, "the overflow menu offers Uninstall");
    await act(async () => item.click());
    assert.deepEqual(calls, [], "choosing Uninstall alone never removes the plugin");
    const dialog = document.querySelector<HTMLElement>('[role="alertdialog"]');
    assert.ok(dialog, "Uninstall asks for confirmation");
    assert.equal(document.activeElement?.textContent, "Cancel", "focus moves to the safe choice");

    const cancel = [...dialog.querySelectorAll("button")].find((button) => button.textContent === "Cancel")!;
    await act(async () => cancel.click());
    assert.equal(document.querySelector('[role="alertdialog"]'), null);
    assert.deepEqual(calls, [], "cancel keeps the plugin");

    const again = await openOverflow(view.host);
    const retry = [...again.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')].find((node) => /uninstall/i.test(node.textContent ?? ""))!;
    await act(async () => retry.click());
    const confirm = [...document.querySelectorAll<HTMLButtonElement>('[role="alertdialog"] button')].find((button) => button.textContent === "Uninstall")!;
    await act(async () => confirm.click());
    assert.deepEqual(calls, [{ kind: "remove", options: { removeData: true } }], "confirming removes exactly once with the data choice");
  } finally {
    await unmount(view);
  }
}

async function configureWithAiIsTheHeaderActionAndFiresOnce() {
  const { __setInvokeForTest } = await import("../../lib/bridge");
  __setInvokeForTest(async <T,>(command: string): Promise<T> => {
    if (command === "get_config") return { api: { host: "127.0.0.1", port: 7686 } } as T;
    throw new Error(`unexpected invoke: ${command}`);
  });
  const requests: string[] = [];
  (globalThis as unknown as { fetch: unknown }).fetch = async (url: string) => {
    requests.push(url);
    return { ok: true, status: 200, json: async () => ({ id: "helper", workspace_id: "general", delivery_state: "launched" }) };
  };
  const calls: Call[] = [];
  const view = await mount(telegram, calls);
  try {
    const buttons = [...view.host.querySelectorAll<HTMLButtonElement>("header button")];
    const configureIndex = buttons.findIndex((button) => button.getAttribute("aria-label") === "Configure Telegram Client with AI");
    const overflowIndex = buttons.findIndex((button) => button.getAttribute("aria-label") === "More actions for Telegram Client");
    assert.ok(configureIndex >= 0, "Configure with AI is a header button");
    assert.ok(configureIndex < overflowIndex, "Configure with AI comes before the overflow in focus order");
    await act(async () => buttons[configureIndex].click());
    await act(async () => { for (let tick = 0; tick < 30 && !requests.length; tick++) await Promise.resolve(); });
    await act(async () => { for (let tick = 0; tick < 10; tick++) await Promise.resolve(); });
    assert.equal(requests.length, 1, "one click starts exactly one configure request");
    assert.deepEqual(calls, []);
  } finally {
    await unmount(view);
    __setInvokeForTest(null);
  }
}

async function uninstalledPluginOffersInstallOnly() {
  const calls: Call[] = [];
  const view = await mount({ ...telegram, installed: false }, calls);
  try {
    const install = [...view.host.querySelectorAll<HTMLButtonElement>("button")].find((button) => button.textContent === "Install");
    assert.ok(install, "an available plugin offers Install");
    assert.equal(view.host.querySelector('button[aria-haspopup="menu"]'), null, "nothing to uninstall, so no overflow menu");
    assert.equal(view.host.querySelector('button[aria-label="Configure Telegram Client with AI"]'), null);
    await act(async () => install.click());
    assert.deepEqual(calls, [{ kind: "install", options: undefined }]);
  } finally {
    await unmount(view);
  }
}

async function pluginNameAppearsOnceOnTheDetailPage() {
  const { PluginDetailPanel } = await import("./PluginDetailPanel");
  const markup = renderToStaticMarkup(<PluginDetailPanel
    entry={{ ...telegram, capabilities: { view: true } }}
    config={{ api: { host: "127.0.0.1", port: 7686 } }}
    busy={null}
    onAction={() => {}}
    onHostGrant={() => {}}
    onSave={async () => {}}
    onUpdateFullConfig={() => {}}
    onGone={() => {}}
    activityDisplayMap={new Map()}
  />);
  const host = document.createElement("div");
  host.innerHTML = markup;
  assert.ok(host.querySelector('[data-testid="extensions-plugin-view"] iframe'), "the plugin view is embedded");
  const occurrences = (host.textContent ?? "").split(telegram.displayName).length - 1;
  assert.equal(occurrences, 1, "the plugin name is shown once, in the page heading");
  assert.equal(host.querySelector("h2")?.textContent, telegram.displayName, "the heading carries the name");
}

async function main() {
  await uninstallLivesInOverflowAndStillConfirms();
  await configureWithAiIsTheHeaderActionAndFiresOnce();
  await uninstalledPluginOffersInstallOnly();
  await pluginNameAppearsOnceOnTheDetailPage();
  console.log("PluginDetailHeader behavior: 4/4 passed");
}

void main().catch((error: unknown) => { console.error(error); process.exitCode = 1; });
