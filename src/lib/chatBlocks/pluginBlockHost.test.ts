// Run: npx tsx src/lib/chatBlocks/pluginBlockHost.test.ts

import { Window } from "happy-dom";
import type { PluginSnapshot } from "../../components/Extensions/types";
import type { PluginSnapshotStoreState } from "../pluginSnapshotStore";
import { createPluginBlockHost } from "./pluginBlockHost";
import { pluginBlockState, pluginPitch } from "./pluginBlockState";
import { buildCatalog } from "../../components/Extensions/pluginCatalog";

const testWindow = new Window({ url: "http://localhost/" });
Object.defineProperty(globalThis, "window", { value: testWindow, configurable: true });
Object.defineProperty(globalThis, "CustomEvent", { value: testWindow.CustomEvent, configurable: true });

const tests: Array<[string, () => void | Promise<void>]> = [];
function test(name: string, run: () => void | Promise<void>) {
  tests.push([name, run]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const SNAPSHOT: PluginSnapshot = {
  plugins: [{ id: "teams-client", display_name: "Teams Client", version: "1.0.0", capabilities: { view: true }, description: "Signs in to Teams. Reads chats." }],
  channels: [],
  diff: [],
  marketplace: [
    { id: "telegram-client", channel: "codeterm-plugins", display_name: "Telegram Client", pitch: "Let agents use your Telegram account.", installed: false, compatible: true } as never,
  ],
};

function storeState(overrides: Partial<PluginSnapshotStoreState> = {}): PluginSnapshotStoreState {
  return { snapshot: SNAPSHOT, lastSuccessfulSnapshot: SNAPSHOT, error: null, loading: false, refreshing: false, hasLoaded: true, lastLoadedAt: 1, ...overrides };
}

function fakeStore(initial = storeState()) {
  let state = initial;
  const listeners = new Set<() => void>();
  const calls = { ensure: 0, actions: [] as Array<Record<string, unknown>> };
  return {
    calls,
    set(next: PluginSnapshotStoreState) {
      state = next;
      for (const listener of listeners) listener();
    },
    store: {
      get: () => state,
      subscribe(listener: () => void) { listeners.add(listener); return () => { listeners.delete(listener); }; },
      async ensure() { calls.ensure++; },
      async runAction(action: Record<string, unknown>) { calls.actions.push(action); return SNAPSHOT; },
    },
  };
}

test("state_is_honest_while_the_plugin_list_is_unknown", () => {
  const catalog = buildCatalog(SNAPSHOT);
  assert(pluginBlockState({ hasLoaded: false, error: null }, [], "telegram-client").kind === "loading", "unknown list claimed a status");
  assert(pluginBlockState({ hasLoaded: false, error: "offline" }, [], "telegram-client").kind === "unavailable", "fetch error hidden");
  assert(pluginBlockState({ hasLoaded: true, error: null }, catalog, "nope").kind === "missing", "unknown id not missing");
  const ready = pluginBlockState({ hasLoaded: true, error: null }, catalog, "teams-client");
  assert(ready.kind === "ready" && ready.plugin.installed && ready.plugin.hasView, "installed plugin facts lost");
  const market = pluginBlockState({ hasLoaded: true, error: null }, catalog, "telegram-client");
  assert(market.kind === "ready" && !market.plugin.installed && market.plugin.pitch === "Let agents use your Telegram account.", "manifest pitch lost");
});

test("pitch_falls_back_to_the_first_description_sentence", () => {
  assert(pluginPitch({ description: "Signs in to Teams. Reads chats." }) === "Signs in to Teams.", "first sentence not used");
  assert(pluginPitch({}) === "", "empty description should give no pitch");
  assert(pluginPitch({ description: "x".repeat(400) }).length <= 160, "pitch not bounded");
});

test("reads_are_stable_and_never_run_actions", () => {
  const { store, calls } = fakeStore();
  const host = createPluginBlockHost(store);
  const first = host.binding.read("telegram-client");
  assert(first === host.binding.read("telegram-client"), "read must be referentially stable between changes");
  host.binding.refresh("telegram-client");
  assert(calls.actions.length === 0 && calls.ensure === 1, "refresh must only re-read");
});

test("reads_follow_store_changes", () => {
  const harness = fakeStore(storeState({ hasLoaded: false, snapshot: { plugins: [], channels: [], diff: [] } }));
  const host = createPluginBlockHost(harness.store);
  let notified = 0;
  const unsubscribe = host.binding.subscribe(() => notified++);
  assert(host.binding.read("telegram-client").kind === "loading", "should load first");
  harness.set(storeState());
  assert(notified === 1 && host.binding.read("telegram-client").kind === "ready", "card did not follow the store");
  unsubscribe();
});

test("install_sends_the_extensions_payload_with_the_owning_channel", async () => {
  const { store, calls } = fakeStore();
  const host = createPluginBlockHost(store);
  await host.run("telegram-client", "install");
  assert(calls.actions.length === 1, "install must call the action once");
  const [action] = calls.actions;
  assert(action.kind === "install" && action.pluginId === "telegram-client" && action.channel === "codeterm-plugins", `wrong payload ${JSON.stringify(action)}`);
  let threw = false;
  await host.run("ghost", "install").catch(() => { threw = true; });
  assert(threw && calls.actions.length === 1, "unknown plugin must not reach the daemon");
});

test("open_routes_through_the_app_panels", () => {
  const { store } = fakeStore();
  const host = createPluginBlockHost(store);
  const panels: string[] = [];
  const onPanel = (event: Event) => panels.push((event as CustomEvent<{ panel: string }>).detail.panel);
  window.addEventListener("codeterm:open-panel", onPanel);
  host.openView("teams-client");
  host.openInPlugins("teams-client");
  window.removeEventListener("codeterm:open-panel", onPanel);
  assert(panels.join(",") === "view:teams-client,extensions", `unexpected panels ${panels.join(",")}`);
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
  console.log(`chatBlocks/pluginBlockHost tests: ${failed === 0 ? "all passed" : `${failed} failed`}`);
  process.exit(failed === 0 ? 0 : 1);
})();
