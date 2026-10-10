// Run: node scripts/run-ts-unit-tests.mjs MessengersCard

import { act } from "react";
import { Window } from "happy-dom";
import type { AppConfig } from "../../lib/types";
import type { TelegramBridgeRaw } from "../../lib/messengers/telegramBridge";
import type { TelegramBridgeSnapshot, TelegramBridgeStore } from "../../lib/messengers/telegramBridgeStore";

function assert(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}

const testWindow = new Window({ url: "http://localhost/" });
Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { value: true, configurable: true });
for (const key of ["window", "document", "navigator", "localStorage", "HTMLElement", "HTMLInputElement", "HTMLButtonElement", "HTMLFormElement", "SVGElement", "Element", "Node", "Event", "SubmitEvent"] as const) {
  Object.defineProperty(globalThis, key, { value: key === "window" ? testWindow : testWindow[key], configurable: true });
}
testWindow.matchMedia = (() => ({
  matches: false, media: "", onchange: null,
  addEventListener: () => {}, removeEventListener: () => {},
  addListener: () => {}, removeListener: () => {}, dispatchEvent: () => false,
})) as unknown as typeof testWindow.matchMedia;

const TOKEN = "123456789:AAEhBOweik6ad9r_QXMENQjcrGbqCr4K-ab";

function raw(overrides: Partial<TelegramBridgeRaw> = {}): TelegramBridgeRaw {
  return { active: false, config_enabled: false, token_present: false, has_secret_in_vault: false, secret_name: "telegram_bot_token", ...overrides };
}

function fakeStore(initial: TelegramBridgeRaw) {
  let snapshot: TelegramBridgeSnapshot = { kind: "ready", raw: initial };
  const listeners = new Set<() => void>();
  const calls = { connect: [] as Array<string | undefined>, disconnect: 0 };
  const publish = (next: TelegramBridgeRaw) => {
    snapshot = { kind: "ready", raw: next };
    for (const listener of listeners) listener();
  };
  const store: TelegramBridgeStore = {
    get: () => snapshot,
    subscribe(listener) { listeners.add(listener); return () => { listeners.delete(listener); }; },
    refresh: async () => {},
    async connect(token) {
      calls.connect.push(token);
      publish(raw({ active: true, started_at: "a", last_poll_ok: "b", bot_id: 42, has_secret_in_vault: true }));
    },
    async disconnect() {
      calls.disconnect++;
      publish(raw({ has_secret_in_vault: true }));
    },
  };
  return { store, calls };
}

const CONFIG = { integrations: { telegram_enabled: false, telegram_allowed_users: [] } } as unknown as AppConfig;

(async () => {
  const { createRoot } = await import("react-dom/client");
  const { MessengersCard } = await import("./MessengersCard");
  let failed = 0;
  const run = async (name: string, initial: TelegramBridgeRaw, body: (host: HTMLElement, calls: ReturnType<typeof fakeStore>["calls"]) => Promise<void>) => {
    const { store, calls } = fakeStore(initial);
    const host = document.createElement("div");
    document.body.appendChild(host);
    const root = createRoot(host);
    try {
      await act(async () => root.render(<MessengersCard config={CONFIG} onUpdate={() => {}} store={store} />));
      await body(host, calls);
      console.log(`  ✓ ${name}`);
    } catch (error) {
      failed++;
      console.error(`  ✗ ${name}: ${error instanceof Error ? error.message : error}`);
    } finally {
      await act(async () => root.unmount());
      host.remove();
    }
  };
  const byTestId = (host: HTMLElement, id: string) => host.querySelector(`[data-testid="${id}"]`) as HTMLElement | null;

  await run("token_is_entered_masked_and_connect_runs_once", raw(), async (host, calls) => {
    const token = byTestId(host, "settings-telegram-token") as HTMLInputElement | null;
    assert(token?.type === "password", "token field missing or not masked");
    assert(calls.connect.length < 1, "rendering connected");
    token.value = TOKEN;
    await act(async () => (byTestId(host, "settings-telegram-connect") as HTMLButtonElement).click());
    assert(calls.connect.length === 1 && calls.connect[0] === TOKEN, `connect calls ${calls.connect.length}`);
    assert(byTestId(host, "settings-telegram-connection")?.getAttribute("data-phase") === "connected", "card did not follow the store");
    assert(host.textContent?.includes("Connected as bot 42."), "bot identity not shown");
    assert(!host.innerHTML.includes(TOKEN), "token leaked into the DOM");
  });

  await run("a_saved_token_connects_without_entry", raw({ token_present: true }), async (host, calls) => {
    assert(!byTestId(host, "settings-telegram-token"), "saved token still asks for entry");
    await act(async () => (byTestId(host, "settings-telegram-connect") as HTMLButtonElement).click());
    assert(calls.connect.length === 1 && calls.connect[0] === undefined, "saved-token connect sent a token");
  });

  await run("disconnect_asks_first", raw({ active: true, started_at: "a", last_poll_ok: "b", bot_id: 42, has_secret_in_vault: true }), async (host, calls) => {
    await act(async () => (byTestId(host, "settings-telegram-disconnect") as HTMLButtonElement).click());
    assert(calls.disconnect < 1, "disconnect ran without confirmation");
    await act(async () => (byTestId(host, "settings-telegram-disconnect-confirm") as HTMLButtonElement).click());
    assert(calls.disconnect === 1, "confirmed disconnect did not run once");
    assert(byTestId(host, "settings-telegram-connection")?.getAttribute("data-phase") === "disconnected", "card did not follow disconnect");
  });

  console.log(`Settings/MessengersCard tests: ${failed === 0 ? "all passed" : `${failed} failed`}`);
  process.exit(failed === 0 ? 0 : 1);
})();
