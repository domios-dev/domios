// Run: npx tsx src/lib/messengers/telegramBridge.test.ts

import { Window } from "happy-dom";
import { createConnectBlockHost } from "../chatBlocks/connectBlockHost";
import { connectTelegram, telegramConnectInfo, telegramPhase, telegramTokenProblem, type TelegramBridgeApi, type TelegramBridgeRaw } from "./telegramBridge";
import { createTelegramBridgeStore } from "./telegramBridgeStore";

const testWindow = new Window({ url: "http://localhost/" });
Object.defineProperty(globalThis, "window", { value: testWindow, configurable: true });

const tests: Array<[string, () => void | Promise<void>]> = [];
function test(name: string, run: () => void | Promise<void>) {
  tests.push([name, run]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const TOKEN = "123456789:AAEhBOweik6ad9r_QXMENQjcrGbqCr4K-ab";

function raw(overrides: Partial<TelegramBridgeRaw> = {}): TelegramBridgeRaw {
  return { active: false, config_enabled: false, token_present: false, has_secret_in_vault: false, secret_name: "telegram_bot_token", ...overrides };
}

function fakeApi(initial = raw()) {
  let current = initial;
  const calls: string[] = [];
  const api: TelegramBridgeApi = {
    async state() { calls.push("state"); return current; },
    async saveToken(name, token) { calls.push(`save:${name}:${token === TOKEN}`); current = { ...current, has_secret_in_vault: true }; },
    async start(replace) {
      calls.push(`start:${replace}`);
      current = { ...current, active: true, config_enabled: true, started_at: "2026-10-08T10:00:00Z", last_poll_ok: "2026-10-08T10:00:01Z", bot_id: 123456789 };
    },
    async stop() { calls.push("stop"); current = { ...current, active: false, config_enabled: false }; },
  };
  return { api, calls };
}

test("phase_follows_the_supervisor_and_its_last_poll", () => {
  assert(telegramPhase(raw()) === "disconnected", "inactive must be disconnected");
  assert(telegramPhase(raw({ active: true })) === "connecting", "no poll yet must be connecting");
  assert(telegramPhase(raw({ active: true, started_at: "2026-10-08T10:00:00Z", last_poll_ok: "2026-10-08T10:00:01Z" })) === "connected", "fresh poll must be connected");
  assert(telegramPhase(raw({ active: true, started_at: "2026-10-08T10:00:00Z", last_poll_ok: "2026-10-08T09:00:00Z" })) === "connecting", "a poll from before the start does not prove the connection");
  assert(telegramPhase(raw({ active: true, last_poll_error: "401 Unauthorized" })) === "failing", "poll error must be failing");
});

test("info_names_the_bot_and_never_carries_a_token", () => {
  const info = telegramConnectInfo(raw({ active: true, last_poll_ok: "x", bot_id: 42, has_secret_in_vault: true }));
  assert(info.identity === "bot 42" && info.tokenStored && info.method.kind === "token", "identity or token presence lost");
  assert(telegramConnectInfo(raw({ token_present: true })).tokenStored, "token file not counted as stored");
  assert(telegramConnectInfo(raw()).identity === null, "identity guessed without a token");
});

test("tokens_are_checked_before_anything_is_saved", async () => {
  assert(telegramTokenProblem(TOKEN) === null, "valid token rejected");
  for (const bad of ["", "hello", "123:short", "abc:AAEhBOweik6ad9r_QXMENQjcrGbqCr4K-ab"]) assert(telegramTokenProblem(bad), `accepted ${bad}`);
  const { api, calls } = fakeApi();
  let threw = false;
  await connectTelegram(api, "nope").catch(() => { threw = true; });
  assert(threw && calls.length === 0, "an invalid token reached the daemon");
});

test("connect_saves_to_the_secret_store_before_starting_and_replaces_the_old_file", async () => {
  const { api, calls } = fakeApi();
  await connectTelegram(api, `  ${TOKEN}  `);
  assert(calls.join(",") === "save:telegram_bot_token:true,start:true", `calls ${calls.join(",")}`);
  const saved = fakeApi(raw({ token_present: true }));
  await connectTelegram(saved.api);
  assert(saved.calls.join(",") === "start:false", "a saved-token connect must not save or replace");
});

test("store_rereads_on_config_change_and_after_its_own_actions", async () => {
  const { api, calls } = fakeApi();
  let configListener: () => void = () => {};
  const store = createTelegramBridgeStore(api, (listener) => { configListener = listener; return () => {}; });
  let notified = 0;
  const unsubscribe = store.subscribe(() => notified++);
  assert(store.get().kind === "loading", "store claimed a state before reading");
  await store.refresh();
  assert(store.get().kind === "ready" && calls.join(",") === "state", "refresh must only read");
  configListener();
  await store.refresh();
  assert(calls.filter((call) => call === "state").length >= 2, "config change did not re-read");
  await store.connect(TOKEN);
  const after = store.get();
  assert(after.kind === "ready" && telegramPhase(after.raw) === "connected" && notified > 0, "store did not follow connect");
  await store.disconnect();
  const stopped = store.get();
  assert(stopped.kind === "ready" && !stopped.raw.active, "store did not follow disconnect");
  unsubscribe();
});

test("connect_host_derives_stable_states_and_refuses_unknown_bridges", async () => {
  const { api, calls } = fakeApi(raw({ token_present: true }));
  const store = createTelegramBridgeStore(api, () => () => {});
  const opened: string[] = [];
  const host = createConnectBlockHost({ telegram: { store, describe: telegramConnectInfo } }, (id) => opened.push(id));
  host.binding.subscribe(() => {});
  assert(host.binding.read("telegram").kind === "loading", "unknown state claimed");
  await store.refresh();
  const ready = host.binding.read("telegram");
  assert(ready.kind === "ready" && ready === host.binding.read("telegram"), "read must be stable between changes");
  assert(host.binding.read("discord").kind === "unsupported", "unknown bridge not refused");
  let threw = false;
  await host.connect("discord").catch(() => { threw = true; });
  assert(threw && !calls.some((call) => call.startsWith("start")), "unknown bridge reached the daemon");
  await host.connect("telegram");
  assert(calls.filter((call) => call.startsWith("start")).length === 1, "connect did not run once");
  host.openSettings("telegram");
  assert(opened.join(",") === "telegram", "settings link not routed");
});

(async () => {
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
  console.log(`messengers/telegramBridge tests: ${failed === 0 ? "all passed" : `${failed} failed`}`);
  process.exit(failed === 0 ? 0 : 1);
})();
