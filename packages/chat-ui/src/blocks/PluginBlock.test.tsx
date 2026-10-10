// Run: npx tsx packages/chat-ui/src/blocks/PluginBlock.test.tsx

import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import type { PluginBlockInfo, PluginBlockState, PluginLifecycleKind } from "./pluginBlockModel";

const tests: Array<[string, () => Promise<void>]> = [];
function test(name: string, run: () => Promise<void>) {
  tests.push([name, run]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const testWindow = new Window({ url: "http://localhost/" });
Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { value: true, configurable: true });
for (const key of ["window", "document", "navigator", "HTMLElement", "HTMLButtonElement", "SVGElement", "Element", "Node", "Event"] as const) {
  Object.defineProperty(globalThis, key, { value: key === "window" ? testWindow : testWindow[key], configurable: true });
}
testWindow.matchMedia = (() => ({
  matches: false, media: "", onchange: null,
  addEventListener: () => {}, removeEventListener: () => {},
  addListener: () => {}, removeListener: () => {}, dispatchEvent: () => false,
})) as unknown as typeof testWindow.matchMedia;

function info(overrides: Partial<PluginBlockInfo> = {}): PluginBlockInfo {
  return {
    id: "telegram-client", name: "Telegram Client", pitch: "Let agents use your Telegram account.",
    installed: false, updateAvailable: false, compatible: true, managed: false, hasView: true, pendingReload: false,
    ...overrides,
  };
}

const MISSING: PluginBlockState = { kind: "missing" };

function fakeHost(initial: Record<string, PluginBlockState>) {
  const states = new Map(Object.entries(initial));
  const listeners = new Set<() => void>();
  const calls = { run: [] as Array<[string, PluginLifecycleKind]>, refresh: [] as string[], openView: [] as string[], openInPlugins: [] as string[] };
  let release: () => void = () => {};
  const host = {
    binding: {
      subscribe(listener: () => void) { listeners.add(listener); return () => { listeners.delete(listener); }; },
      read: (id: string): PluginBlockState => states.get(id) ?? MISSING,
      refresh: (id: string) => { calls.refresh.push(id); },
    },
    run(id: string, kind: PluginLifecycleKind) {
      calls.run.push([id, kind]);
      return new Promise<void>((resolve) => {
        release = () => {
          const current = states.get(id);
          if (current?.kind === "ready") states.set(id, { kind: "ready", plugin: { ...current.plugin, installed: true, updateAvailable: false } });
          for (const listener of listeners) listener();
          resolve();
        };
      });
    },
    openView: (id: string) => { calls.openView.push(id); },
    openInPlugins: (id: string) => { calls.openInPlugins.push(id); },
  };
  return { host, calls, finish: () => release() };
}

(async () => {
  const { RichMarkdown } = await import("../MarkdownBody");
  const { ChatBlockHostProvider } = await import("./ChatBlockHost");
  type Host = Parameters<typeof ChatBlockHostProvider>[0]["value"];
  const menuButtons: Host["renderMenu"] = ({ items }) => items.map((item) => <button key={item.label} type="button" aria-label={item.label} onClick={item.onSelect} />);

  async function withRender(text: string, value: Host, run: (host: HTMLDivElement) => Promise<void>) {
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
  const action = (element: HTMLElement, id: string) => element.querySelector(`[data-block-action="${id}"]`) as HTMLButtonElement | null;
  const status = (element: HTMLElement) => element.querySelector('[data-testid="chat-block-status"]')?.textContent;

  test("card_states_follow_the_host_and_rendering_never_mutates", async () => {
    const { host, calls } = fakeHost({
      "telegram-client": { kind: "ready", plugin: info() },
      "teams-client": { kind: "ready", plugin: info({ id: "teams-client", name: "Teams Client", installed: true }) },
      git: { kind: "ready", plugin: info({ id: "git", name: "Git", installed: true, updateAvailable: true, remoteVersion: "2.0.0" }) },
    });
    const text = "@ds-plugin telegram-client\n\n@ds-plugin teams-client\n\n@ds-plugin git\n\n@ds-plugin nope-404";
    await withRender(text, { plugins: host }, async (element) => {
      const cards = [...element.querySelectorAll('[data-chat-block="plugin"]')] as HTMLElement[];
      assert(cards.length === 4, `expected 4 plugin blocks, got ${cards.length}`);
      assert(status(cards[0]) === "Not installed" && action(cards[0], "install"), "not-installed card lacks Install");
      assert(status(cards[1]) === "Installed" && action(cards[1], "open") && !action(cards[1], "install"), "installed card should Open");
      assert(status(cards[2]) === "Update available" && action(cards[2], "update"), "update card lacks Update");
      assert(cards[3].getAttribute("data-block-missing") === "true" && !cards[3].querySelector("button"), "unknown id must be a muted not-found card");
      assert(calls.run.length === 0, "rendering ran a lifecycle action");
      assert(["telegram-client", "teams-client", "git", "nope-404"].every((id) => calls.refresh.includes(id)), "cards must re-read state on mount");
    });
  });

  test("install_runs_once_on_click_and_reports_the_result_in_place", async () => {
    const { host, calls, finish } = fakeHost({ "telegram-client": { kind: "ready", plugin: info() } });
    await withRender("@ds-plugin telegram-client", { plugins: host }, async (element) => {
      const install = action(element, "install");
      assert(install && !install.disabled, "Install not clickable");
      await act(async () => { install.click(); install.click(); });
      assert(calls.run.length === 1 && calls.run[0][0] === "telegram-client" && calls.run[0][1] === "install", `install ran ${calls.run.length} times`);
      assert(action(element, "install")?.getAttribute("aria-busy") === "true", "progress not shown while installing");
      await act(async () => finish());
      assert(status(element) === "Installed", "status did not follow the host after install");
      assert(element.querySelector('[data-testid="chat-block-result"]')?.textContent?.startsWith("Installed"), "result not shown in place");
      assert(calls.run.length === 1, "install ran again after completion");
    });
  });

  test("secondary_open_in_plugins_and_open_view_route_to_the_host", async () => {
    const { host, calls } = fakeHost({ "telegram-client": { kind: "ready", plugin: info({ installed: true }) } });
    await withRender("@ds-plugin telegram-client", { plugins: host }, async (element) => {
      const openInPlugins = [...element.querySelectorAll("button")].find((button) => button.textContent === "Open in Plugins");
      await act(async () => { openInPlugins?.click(); action(element, "open")?.click(); });
      assert(calls.openInPlugins[0] === "telegram-client" && calls.openView[0] === "telegram-client", "open actions not routed");
      assert(calls.run.length === 0, "open must not install");
    });
  });

  test("extras_appear_only_when_the_scope_provides_them", async () => {
    const { host } = fakeHost({ "telegram-client": { kind: "ready", plugin: info() } });
    const asked: string[] = [];
    const pinned: string[] = [];
    await withRender("@ds-plugin telegram-client", { plugins: host }, async (element) => {
      assert(!element.querySelector('[aria-label^="Ask about"]') && !element.querySelector('[aria-label^="Pin "]'), "extras shown without a tab scope");
      assert(!element.querySelector('[aria-label^="Copy link"]'), "the frame offers no copy-link extra");
    });
    await withRender("@ds-plugin telegram-client", { plugins: host, renderMenu: menuButtons, askAbout: ({ text }) => asked.push(text), pin: (source) => pinned.push(source) }, async (element) => {
      await act(async () => {
        (element.querySelector('[aria-label^="Ask about"]') as HTMLButtonElement).click();
        (element.querySelector('[aria-label^="Pin "]') as HTMLButtonElement).click();
      });
      assert(asked.length === 1 && asked[0].includes("@ds-plugin telegram-client"), "ask did not reference the block");
      assert(pinned[0] === "@ds-plugin telegram-client", "pin did not receive the block source");
      assert(!element.querySelector('[aria-label^="Copy link"]'), "copy link must not be offered in a tab scope");
    });
  });

  test("risky_actions_confirm_before_running", async () => {
    const { ChatBlockFrame } = await import("./ChatBlockFrame");
    let ran = 0;
    const element = document.createElement("div");
    document.body.appendChild(element);
    const root = createRoot(element);
    await act(async () => root.render(
      <ChatBlockFrame name="test" source="@ds-test x" title="Test" icon={null}
        actions={[{ id: "remove", label: "Remove", confirm: { message: "Remove it?", confirmLabel: "Remove now" }, run: () => { ran++; } }]} />,
    ));
    await act(async () => action(element, "remove")?.click());
    assert(ran === 0, "risky action ran without confirmation");
    const confirm = [...element.querySelectorAll("button")].find((button) => button.textContent === "Remove now");
    await act(async () => confirm?.click());
    assert(ran === 1, "confirmed action did not run once");
    await act(async () => root.unmount());
    element.remove();
  });

  test("without_a_plugin_host_the_card_says_so", async () => {
    await withRender("@ds-plugin telegram-client", {}, async (element) => {
      assert(element.querySelector('[data-chat-block="plugin"]') && !element.querySelector("button"), "hostless view must not offer actions");
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
  console.log(`chat-ui/blocks/PluginBlock tests: ${failed === 0 ? "all passed" : `${failed} failed`}`);
  process.exit(failed === 0 ? 0 : 1);
})();
