// Run: npx tsx packages/chat-ui/src/blocks/ConnectBlock.test.tsx

import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import type { BridgeConnectInfo, ConnectBlockState } from "./connectBlockModel";

const tests: Array<[string, () => Promise<void>]> = [];
function test(name: string, run: () => Promise<void>) {
  tests.push([name, run]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const testWindow = new Window({ url: "http://localhost/" });
Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { value: true, configurable: true });
for (const key of ["window", "document", "navigator", "HTMLElement", "HTMLInputElement", "HTMLButtonElement", "SVGElement", "Element", "Node", "Event"] as const) {
  Object.defineProperty(globalThis, key, { value: key === "window" ? testWindow : testWindow[key], configurable: true });
}
testWindow.matchMedia = (() => ({
  matches: false, media: "", onchange: null,
  addEventListener: () => {}, removeEventListener: () => {},
  addListener: () => {}, removeListener: () => {}, dispatchEvent: () => false,
})) as unknown as typeof testWindow.matchMedia;

function info(overrides: Partial<BridgeConnectInfo> = {}): BridgeConnectInfo {
  return {
    id: "telegram", name: "Telegram", description: "Message your General Agent from Telegram.",
    phase: "disconnected", tokenStored: false,
    method: { kind: "token", label: "Bot token", placeholder: "123:AA", help: "Paste the token." },
    ...overrides,
  };
}

function fakeHost(initial: ConnectBlockState) {
  let state = initial;
  const listeners = new Set<() => void>();
  const calls = { connect: [] as Array<string | undefined>, disconnect: 0, refresh: 0, settings: 0 };
  let release: (next: ConnectBlockState) => void = () => {};
  const set = (next: ConnectBlockState) => {
    state = next;
    for (const listener of listeners) listener();
  };
  const host = {
    binding: {
      subscribe(listener: () => void) { listeners.add(listener); return () => { listeners.delete(listener); }; },
      read: () => state,
      refresh: () => { calls.refresh++; },
    },
    connect(_id: string, token?: string) {
      calls.connect.push(token);
      return new Promise<void>((resolve) => {
        release = (next) => { set(next); resolve(); };
      });
    },
    async disconnect() {
      calls.disconnect++;
      set({ kind: "ready", bridge: info({ tokenStored: true }) });
    },
    openSettings: () => { calls.settings++; },
  };
  return { host, calls, set, finish: (next: ConnectBlockState) => release(next) };
}

(async () => {
  const { RichMarkdown } = await import("../MarkdownBody");
  const { ChatBlockHostProvider } = await import("./ChatBlockHost");
  type Host = Parameters<typeof ChatBlockHostProvider>[0]["value"];

  async function withRender(value: Host, run: (element: HTMLDivElement) => Promise<void>) {
    const element = document.createElement("div");
    document.body.appendChild(element);
    const root = createRoot(element);
    try {
      await act(async () => root.render(<ChatBlockHostProvider value={value}><RichMarkdown text="@ds-connect telegram" /></ChatBlockHostProvider>));
      await run(element);
    } finally {
      await act(async () => root.unmount());
      element.remove();
    }
  }
  const action = (element: HTMLElement, id: string) => element.querySelector(`[data-block-action="${id}"]`) as HTMLButtonElement | null;
  const status = (element: HTMLElement) => element.querySelector('[data-testid="chat-block-status"]')?.textContent;
  const button = (element: HTMLElement, text: string) => [...element.querySelectorAll("button")].find((candidate) => candidate.textContent === text);
  const type = async (input: HTMLInputElement, value: string) => {
    await act(async () => {
      input.value = value;
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
  };

  test("without_a_token_the_card_asks_for_one_and_connects_only_on_click", async () => {
    const { host, calls, finish } = fakeHost({ kind: "ready", bridge: info() });
    await withRender({ connect: host }, async (element) => {
      assert(status(element) === "Not connected", `status ${status(element)}`);
      assert(calls.refresh === 1 && calls.connect.length === 0, "mount must only re-read");
      const token = element.querySelector('[data-testid="chat-connect-token"]') as HTMLInputElement | null;
      assert(token && token.type === "password", "token field missing or not masked");
      assert(action(element, "connect")?.disabled, "Connect enabled before a token is pasted");
      await type(token, "123456789:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA");
      const connect = action(element, "connect");
      assert(connect && !connect.disabled, "Connect not enabled after pasting");
      await act(async () => { connect.click(); connect.click(); });
      assert(calls.connect.length === 1 && calls.connect[0] === "123456789:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", `connect ran ${calls.connect.length} times`);
      assert(!element.innerHTML.includes("AAAAAAAAAAAAAAAAAAAA"), "token leaked into the DOM");
      await act(async () => finish({ kind: "ready", bridge: info({ phase: "connected", tokenStored: true, identity: "bot 42" }) }));
      assert(status(element) === "Connected", "status did not follow the host");
      assert(element.querySelector('[data-testid="chat-block-result"]')?.textContent === "Connected as bot 42.", "result not shown in place");
      assert(!element.querySelector('[data-testid="chat-connect-token"]'), "token field stays after connecting");
    });
  });

  test("a_saved_token_connects_without_entry_and_can_be_replaced", async () => {
    const { host, calls, finish } = fakeHost({ kind: "ready", bridge: info({ tokenStored: true }) });
    await withRender({ connect: host }, async (element) => {
      assert(!element.querySelector('[data-testid="chat-connect-token"]'), "saved token still asks for entry");
      await act(async () => action(element, "connect")?.click());
      assert(calls.connect.length === 1 && calls.connect[0] === undefined, "saved-token connect must not send a token");
      await act(async () => finish({ kind: "ready", bridge: info({ tokenStored: true }) }));
      await act(async () => action(element, "replace-token")?.click());
      assert(element.querySelector('[data-testid="chat-connect-token"]'), "replace did not show the field");
      assert(calls.connect.length === 1, "replace must not connect");
    });
  });

  test("connected_is_quiet_and_disconnect_asks_first", async () => {
    const { host, calls } = fakeHost({ kind: "ready", bridge: info({ phase: "connected", tokenStored: true, identity: "bot 42" }) });
    await withRender({ connect: host }, async (element) => {
      assert(status(element) === "Connected" && !action(element, "connect"), "connected card offers Connect");
      assert(element.textContent?.includes("Connected as bot 42."), "identity not shown");
      const disconnect = action(element, "disconnect");
      assert(disconnect, "disconnect missing");
      await act(async () => disconnect.click());
      assert(calls.disconnect === 0, "disconnect ran without confirmation");
      await act(async () => button(element, "Disconnect")?.click());
      assert(calls.disconnect === 1, "confirmed disconnect did not run once");
      assert(status(element) === "Not connected", "status did not follow disconnect");
      await act(async () => button(element, "Open Messengers settings")?.click());
      assert(calls.settings === 1, "full page link not routed");
    });
  });

  test("unknown_bridges_and_hostless_views_offer_no_actions", async () => {
    const { host } = fakeHost({ kind: "unsupported" });
    await withRender({ connect: host }, async (element) => {
      assert(element.querySelector("[data-block-missing]") && !element.querySelector("button"), "unsupported bridge offered actions");
    });
    await withRender({}, async (element) => {
      assert(element.querySelector('[data-chat-block="connect"]') && !element.querySelector("button"), "hostless view offered actions");
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
  console.log(`chat-ui/blocks/ConnectBlock tests: ${failed === 0 ? "all passed" : `${failed} failed`}`);
  process.exit(failed === 0 ? 0 : 1);
})();
