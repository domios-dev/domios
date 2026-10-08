// Run: npx tsx packages/chat-ui/src/blocks/pluginBlockModel.test.ts

import { pluginLifecycleResult, pluginPrimaryAction, pluginStatus, type PluginBlockInfo } from "./pluginBlockModel";

const tests: Array<[string, () => void]> = [];
function test(name: string, run: () => void) {
  tests.push([name, run]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function plugin(overrides: Partial<PluginBlockInfo> = {}): PluginBlockInfo {
  return {
    id: "telegram-client",
    name: "Telegram Client",
    pitch: "",
    installed: false,
    updateAvailable: false,
    compatible: true,
    managed: false,
    hasView: true,
    pendingReload: false,
    ...overrides,
  };
}

test("not_installed_offers_install", () => {
  assert(pluginStatus(plugin()) === "not-installed", "status");
  const action = pluginPrimaryAction(plugin());
  assert(action.kind === "install" && !action.disabled, "install must be enabled");
});

test("incompatible_plugin_blocks_install_with_a_reason", () => {
  const action = pluginPrimaryAction(plugin({ compatible: false, blockedReason: "needs 9.9" }));
  assert(action.kind === "install" && action.disabled && action.reason === "needs 9.9", "blocked install not explained");
});

test("installed_with_update_offers_update", () => {
  const info = plugin({ installed: true, updateAvailable: true });
  assert(pluginStatus(info) === "update" && pluginPrimaryAction(info).kind === "update", "update not offered");
});

test("managed_plugins_never_offer_lifecycle_actions", () => {
  const info = plugin({ installed: true, updateAvailable: true, managed: true });
  assert(pluginStatus(info) === "installed", "managed plugin reported an update");
  assert(pluginPrimaryAction(info).kind === "open", "managed plugin should only open");
});

test("installed_opens_its_view_or_offers_nothing", () => {
  assert(pluginPrimaryAction(plugin({ installed: true })).kind === "open", "view plugin should open");
  assert(pluginPrimaryAction(plugin({ installed: true, hasView: false })).kind === "none", "viewless plugin has no primary");
  const pending = pluginPrimaryAction(plugin({ installed: true, pendingReload: true }));
  assert(pending.kind === "none" && pending.disabled && !!pending.reason, "pending reload must explain why Open is off");
});

test("lifecycle_result_reports_what_the_host_lists_afterwards", () => {
  assert(pluginLifecycleResult("install", { kind: "ready", plugin: plugin({ installed: true }) }).startsWith("Installed"), "install result");
  assert(pluginLifecycleResult("install", { kind: "ready", plugin: plugin({ installed: true, pendingReload: true }) }).includes("Restart"), "reload note");
  assert(!pluginLifecycleResult("install", { kind: "ready", plugin: plugin() }).startsWith("Installed."), "claimed installed while host says not installed");
});

let failed = 0;
for (const [name, run] of tests) {
  try {
    run();
    console.log(`  ✓ ${name}`);
  } catch (error) {
    failed++;
    console.error(`  ✗ ${name}: ${error instanceof Error ? error.message : error}`);
  }
}
console.log(`chat-ui/blocks/pluginBlockModel tests: ${failed === 0 ? "all passed" : `${failed} failed`}`);
process.exit(failed === 0 ? 0 : 1);
