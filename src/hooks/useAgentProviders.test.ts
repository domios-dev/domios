// Run: npx tsx src/hooks/useAgentProviders.test.ts
// Regression for GH #185: secondary useAgentProviders mounts must not wipe the
// shared descriptor cache while their fetch is still in flight.

import {
  getAgentDescriptor,
  setAgentDescriptors,
} from "../lib/agentDescriptorCache";
import { syncAgentDescriptorCache } from "./useAgentProviders";
import type { ProviderDescriptor } from "../lib/types";

const tests: Array<[string, () => void]> = [];
function test(name: string, fn: () => void) {
  tests.push([name, fn]);
}
function assert(cond: unknown, msg: string): asserts cond {
  if (!cond) throw new Error(msg);
}

function provider(id: string): ProviderDescriptor {
  return {
    schema_version: 1,
    id,
    display_name: id,
    transport: "pty",
    system_prompt_delivery: "external",
    available_models: [],
    settings_schema: [],
    notifications_schema: [],
    supports_lazy_skills: false,
    has_hooks: false,
    preserves_context_on_resume: false,
    supports_chat_ui: true,
    icon_hint: id.slice(0, 2).toUpperCase(),
  };
}

test("sync skips while loading so a mounting hook cannot clear warm cache", () => {
  setAgentDescriptors([provider("claude")]);
  syncAgentDescriptorCache([], true);
  assert(getAgentDescriptor("claude")?.id === "claude", "warm cache survives in-flight fetch");
});

test("sync publishes once loading finishes", () => {
  syncAgentDescriptorCache([provider("codex")], false);
  assert(getAgentDescriptor("codex")?.id === "codex", "loaded providers reach cache");
});

test("sync allows empty registry after a completed load", () => {
  syncAgentDescriptorCache([], false);
  assert(getAgentDescriptor("codex") === undefined, "empty completed fetch clears cache");
});

let failed = 0;
for (const [name, fn] of tests) {
  try {
    fn();
    console.log(`✓ ${name}`);
  } catch (err) {
    failed += 1;
    console.error(`✗ ${name}`);
    console.error(err);
  }
}
console.log(`useAgentProviders: ${tests.length - failed}/${tests.length} passed`);
if (failed > 0) process.exit(1);
