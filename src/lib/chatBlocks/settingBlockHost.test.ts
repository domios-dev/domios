// Run: npx tsx src/lib/chatBlocks/settingBlockHost.test.ts

import { Window } from "happy-dom";
import type { PluginSnapshot } from "../../components/Extensions/types";
import { configWrite, createAppConfigSource } from "../appConfigSource";
import type { PluginSnapshotStoreState } from "../pluginSnapshotStore";
import type { AppConfig, ProviderDescriptor, SettingField } from "../types";
import { createSettingBlockHost } from "./settingBlockHost";
import { settingBlockState, type SchemaLookup } from "./settingBlockState";

const testWindow = new Window({ url: "http://localhost/" });
Object.defineProperty(globalThis, "window", { value: testWindow, configurable: true });

const tests: Array<[string, () => void | Promise<void>]> = [];
function test(name: string, run: () => void | Promise<void>) {
  tests.push([name, run]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const SCHEMA: SettingField[] = [{
  kind: "section",
  title: "Chat preprocessing",
  fields: [
    { kind: "toggle", key: "enabled", label: "Enable preprocessing", default: false, description: "When off, messages pass through unchanged." },
    { kind: "number", key: "memK", label: "Memory hits", default: 3, min: 1, max: 8 },
    { kind: "select", key: "insertPosition", label: "Insert position", default: "prepend", options: [["prepend", "Before message"], ["append", "After message"]] },
    { kind: "text", key: "greeting", label: "Greeting", placeholder: "Hi" },
    { kind: "api_key", env_var: "OPENAI_KEY", label: "OpenAI key" },
    { kind: "info", body: "note", level: "info" } as SettingField,
  ],
}];

const ready = (schema: SettingField[] = SCHEMA): SchemaLookup => ({ status: "ready", owner: "Chat Preprocessor", schema });

function state(key: string, config: unknown = { plugins: {} }, lookup: SchemaLookup = ready()) {
  return settingBlockState({ key, config, configError: null, lookup: () => lookup });
}

test("schema_fields_map_to_controls_with_label_description_and_value", () => {
  const toggle = state("plugins.chat-preprocessor.enabled", { plugins: { "chat-preprocessor": { enabled: true } } });
  assert(toggle.kind === "ready" && toggle.setting.control.kind === "switch" && toggle.setting.value === true, "toggle lost");
  assert(toggle.setting.label === "Enable preprocessing" && toggle.setting.description?.startsWith("When off"), "label or description lost");
  const number = state("plugins.chat-preprocessor.memK");
  assert(number.kind === "ready" && number.setting.control.kind === "stepper" && number.setting.value === 3, "number default lost");
  const choice = state("plugins.chat-preprocessor.insertPosition");
  assert(choice.kind === "ready" && choice.setting.control.kind === "choice" && choice.setting.control.options[1].label === "After message", "options lost");
  const text = state("plugins.chat-preprocessor.greeting");
  assert(text.kind === "ready" && text.setting.control.kind === "text" && text.setting.value === "", "text lost");
});

test("secrets_unknown_keys_and_non_value_fields_are_refused", () => {
  const refused = (key: string) => {
    const result = state(key);
    return result.kind === "refused" ? result.reason : result.kind;
  };
  assert(refused("plugins.chat-preprocessor.OPENAI_KEY") === "sensitive", "schema api_key field not refused as sensitive");
  assert(refused("plugins.chat-preprocessor.token") === "sensitive", "token key not refused");
  assert(refused("api.token") === "sensitive" && refused("integrations.secrets.backend") === "sensitive", "core secrets not refused");
  assert(refused("plugins.chat-preprocessor.nope") === "unknown", "undeclared key not refused");
  assert(refused("notifications.desktop") === "unknown", "key without a schema not refused");
  assert(refused("plugins.chat-preprocessor") === "unknown", "partial key not refused");
  const missing = settingBlockState({ key: "plugins.ghost.enabled", config: {}, configError: null, lookup: () => ({ status: "missing" }) });
  assert(missing.kind === "refused" && missing.reason === "unknown", "missing plugin not refused");
});

test("state_is_honest_while_schema_or_config_is_unknown", () => {
  assert(settingBlockState({ key: "plugins.x.enabled", config: null, configError: null, lookup: () => ({ status: "loading" }) }).kind === "loading", "schema loading hidden");
  assert(state("plugins.chat-preprocessor.enabled", null).kind === "loading", "config loading hidden");
  assert(settingBlockState({ key: "plugins.chat-preprocessor.enabled", config: null, configError: "offline", lookup: () => ready() }).kind === "unavailable", "config error hidden");
});

test("a_write_under_a_missing_parent_patches_its_first_missing_ancestor", () => {
  const config = { plugins: { other: {} } };
  const fresh = configWrite(config, "plugins.chat-preprocessor.enabled", true);
  assert(fresh.key === "plugins.chat-preprocessor" && JSON.stringify(fresh.value) === '{"enabled":true}', `fresh ${JSON.stringify(fresh)}`);
  const existing = configWrite({ plugins: { "chat-preprocessor": { memK: 2 } } }, "plugins.chat-preprocessor.enabled", true);
  assert(existing.key === "plugins.chat-preprocessor.enabled" && existing.value === true, "existing parent not written by key");
  const root = configWrite({}, "plugins.x.enabled", false);
  assert(root.key === "plugins" && JSON.stringify(root.value) === '{"x":{"enabled":false}}', "missing root not nested");
});

function pluginState(): PluginSnapshotStoreState {
  const snapshot: PluginSnapshot = {
    plugins: [{ id: "chat-preprocessor", display_name: "Chat Preprocessor", version: "1.0.0", capabilities: {}, settings_schema: SCHEMA } as never],
    channels: [], diff: [],
  };
  return { snapshot, lastSuccessfulSnapshot: snapshot, error: null, loading: false, refreshing: false, hasLoaded: true, lastLoadedAt: 1 };
}

function harness(initial: Record<string, unknown> = { api: {}, plugins: {} }) {
  let emit: (config: AppConfig) => void = () => {};
  const patches: Array<[string, unknown]> = [];
  const config = createAppConfigSource({
    load: async () => initial as unknown as AppConfig,
    onChange: async (listener) => { emit = listener; return () => {}; },
    patch: async (_config, key, value) => { patches.push([key, value]); },
  });
  const plugins = pluginState();
  const opened: string[] = [];
  const agents: ProviderDescriptor[] = [{ id: "claude", display_name: "Claude Code", settings_schema: [{ kind: "toggle", key: "fast", label: "Fast mode", default: false }] } as unknown as ProviderDescriptor];
  const host = createSettingBlockHost({
    config,
    plugins: { get: () => plugins, subscribe: () => () => {}, ensure: async () => {} },
    agents: { list: () => agents, subscribe: () => () => {}, ensure: async () => {} },
    openPlugin: (id) => opened.push(id),
  });
  return { host, config, patches, opened, emit: (next: Record<string, unknown>) => emit(next as unknown as AppConfig) };
}

test("reads_are_stable_follow_config_changes_and_never_write", async () => {
  const { host, config, patches, emit } = harness();
  let notified = 0;
  const unsubscribe = host.binding.subscribe(() => notified++);
  await config.ensure();
  const key = "plugins.chat-preprocessor.enabled";
  const first = host.binding.read(key);
  assert(first.kind === "ready" && first.setting.value === false && first === host.binding.read(key), "read must be stable between changes");
  emit({ api: {}, plugins: { "chat-preprocessor": { enabled: true } } });
  const after = host.binding.read(key);
  assert(notified > 0 && after.kind === "ready" && after.setting.value === true, "a change made elsewhere did not show");
  const agent = host.binding.read("agents.claude.fast");
  assert(agent.kind === "ready" && agent.setting.owner === "Claude Code", "agent provider schema not used");
  assert(patches.length === 0, "reading wrote config");
  unsubscribe();
});

test("write_takes_the_config_set_path_once_and_refuses_what_the_card_cannot_offer", async () => {
  const { host, config, patches, opened } = harness();
  host.binding.subscribe(() => {});
  await config.ensure();
  await host.write("plugins.chat-preprocessor.memK", 5);
  assert(patches.length === 1 && patches[0][0] === "plugins.chat-preprocessor" && JSON.stringify(patches[0][1]) === '{"memK":5}', `patch ${JSON.stringify(patches)}`);
  for (const [key, value] of [["plugins.chat-preprocessor.memK", 99], ["plugins.chat-preprocessor.insertPosition", "middle"], ["plugins.chat-preprocessor.enabled", "yes"], ["api.token", "x"], ["plugins.chat-preprocessor.nope", true]] as const) {
    let refused = false;
    await host.write(key, value).catch(() => { refused = true; });
    assert(refused, `wrote ${key}=${String(value)}`);
  }
  assert(patches.length === 1, "a refused write reached the config");
  host.openSettings?.("plugins.chat-preprocessor.memK");
  assert(opened.join(",") === "chat-preprocessor", "open settings did not target the owning plugin");
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
  console.log(`chatBlocks/settingBlockHost tests: ${failed === 0 ? "all passed" : `${failed} failed`}`);
  process.exit(failed === 0 ? 0 : 1);
})();
