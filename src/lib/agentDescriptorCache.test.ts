// Run: npx tsx src/lib/agentDescriptorCache.test.ts
// defaultSpawnProviderId() backs every "spawn the default agent" affordance

import {
  setAgentDescriptors,
  listAgentDescriptors,
  defaultSpawnProviderId,
} from "./agentDescriptorCache";
import type { ProviderDescriptor } from "./types";

const tests: Array<[string, () => void]> = [];
function test(name: string, fn: () => void) {
  tests.push([name, fn]);
}
function assert(cond: unknown, msg: string): asserts cond {
  if (!cond) throw new Error(msg);
}

function provider(id: string, transport: ProviderDescriptor["transport"]): ProviderDescriptor {
  return {
    schema_version: 1,
    id,
    display_name: id[0].toUpperCase() + id.slice(1),
    transport,
    system_prompt_delivery: "external",
    available_models: [],
    settings_schema: [],
    notifications_schema: [],
    supports_lazy_skills: false,
    has_hooks: false,
  };
}

test("empty registry falls back to claude while descriptors load", () => {
  setAgentDescriptors([]);
  assert(defaultSpawnProviderId() === "claude", "empty cache -> claude fallback");
  assert(listAgentDescriptors().length === 0, "empty list");
});

test("picks the first PTY agent in registry order, not a vendor id", () => {
  setAgentDescriptors([provider("claude", "pty"), provider("codex", "pty")]);
  assert(defaultSpawnProviderId() === "claude", "claude is first pty -> default");
});

test("a different first PTY agent becomes the default", () => {
  setAgentDescriptors([provider("codex", "pty"), provider("claude", "pty")]);
  assert(defaultSpawnProviderId() === "codex", "registry order governs, not the id");
});

test("skips chat_backend providers — spawn defaults are PTY agents", () => {
  setAgentDescriptors([provider("lmstudio", "chat_backend"), provider("opencode", "pty")]);
  assert(defaultSpawnProviderId() === "opencode", "first pty after a chat_backend");
});

test("no PTY agent registered falls back to claude", () => {
  setAgentDescriptors([provider("lmstudio", "chat_backend")]);
  assert(defaultSpawnProviderId() === "claude", "no pty -> claude fallback");
});

test("listAgentDescriptors preserves registry order and identity", () => {
  const list = [provider("claude", "pty"), provider("codex", "pty")];
  setAgentDescriptors(list);
  const a = listAgentDescriptors();
  const b = listAgentDescriptors();
  assert(a === b, "stable identity between calls (useSyncExternalStore safe)");
  assert(a.map((p) => p.id).join(",") === "claude,codex", "preserves order");
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
console.log(`agentDescriptorCache: ${tests.length - failed}/${tests.length} passed`);
if (failed > 0) process.exit(1);
