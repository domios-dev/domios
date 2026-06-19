// Run: npx tsx src/lib/paneProvider.test.ts
//
// resolvePaneProvider unifies pane identity across the two sources the UI has:
// agent panes carry an `agent_type` resolved via the ProviderDescriptor cache,
// shell panes carry `backing.plugin_id` resolved via the plugin-snapshot cache.

import { setAgentDescriptors } from "./agentDescriptorCache";
import { setPluginProviderMeta } from "./pluginProviderMeta";
import { resolvePaneProvider } from "./paneProvider";
import type { Pane, ProviderDescriptor } from "./types";

const tests: Array<[string, () => void]> = [];
function test(name: string, fn: () => void) {
  tests.push([name, fn]);
}
function assert(cond: unknown, msg: string): asserts cond {
  if (!cond) throw new Error(msg);
}

const claudeDescriptor: ProviderDescriptor = {
  schema_version: 1,
  id: "claude",
  display_name: "Claude",
  transport: "pty",
  system_prompt_delivery: "external",
  icon_hint: "C",
  icon_svg: "<svg id='claude'/>",
  icon_color: "#cc785c",
  available_models: [],
  settings_schema: [],
  notifications_schema: [],
  supports_lazy_skills: false,
  has_hooks: false,
  preserves_context_on_resume: false,
};

function seed() {
  setAgentDescriptors([claudeDescriptor]);
  setPluginProviderMeta([
    {
      id: "lmstudio",
      displayName: "LM Studio",
      iconSvg: "<svg id='lmstudio'/>",
      iconColor: "#ff6600",
      iconHint: "LM",
      availableModels: [{ id: "qwen-2.5", display_name: "Qwen 2.5" }],
      defaultModel: "qwen-2.5",
    },
  ]);
}

function pane(overrides: Partial<Pane>): Pane {
  return {
    id: "p1",
    pty_id: null,
    pane_type: "Terminal",
    title: "t",
    cwd: null,
    backing: { kind: "pty" },
    ...overrides,
  };
}

test("resolves an agent pane from the descriptor cache", () => {
  seed();
  const r = resolvePaneProvider(pane({ agent_type: "claude" }));
  assert(r, "expected a resolved provider");
  assert(r.kind === "agent", `expected agent kind, got ${r.kind}`);
  assert(r.id === "claude", `expected id claude, got ${r.id}`);
  assert(r.displayName === "Claude", `expected Claude, got ${r.displayName}`);
  assert(r.iconSvg === "<svg id='claude'/>", "agent icon svg from descriptor");
  assert(r.iconColor === "#cc785c", "agent icon color from descriptor");
  assert(r.iconHint === "C", "agent icon hint from descriptor");
});

test("resolves a shell pane from the plugin snapshot cache", () => {
  seed();
  const r = resolvePaneProvider(
    pane({ backing: { kind: "chat_backend", plugin_id: "lmstudio", session_id: "s1" } }),
  );
  assert(r, "expected a resolved provider");
  assert(r.kind === "shell", `expected shell kind, got ${r.kind}`);
  assert(r.id === "lmstudio", `expected id lmstudio, got ${r.id}`);
  assert(r.displayName === "LM Studio", `expected LM Studio, got ${r.displayName}`);
  assert(r.iconSvg === "<svg id='lmstudio'/>", "shell icon svg from plugin snapshot");
  assert(r.iconColor === "#ff6600", "shell icon color from plugin snapshot");
});

test("shell pane still resolves as shell before its meta is cached", () => {
  setAgentDescriptors([claudeDescriptor]);
  setPluginProviderMeta([]); // snapshot not loaded yet
  const r = resolvePaneProvider(
    pane({ backing: { kind: "chat_backend", plugin_id: "lmstudio", session_id: "s1" } }),
  );
  assert(r, "expected a resolved provider even without cached meta");
  assert(r.kind === "shell", `expected shell kind, got ${r.kind}`);
  assert(r.id === "lmstudio", "id falls back to plugin_id");
  assert(r.displayName === "lmstudio", "displayName falls back to plugin_id");
  assert(!r.iconSvg, "no icon svg until snapshot loads");
});

test("returns null for a plain terminal pane", () => {
  seed();
  assert(resolvePaneProvider(pane({})) === null, "plain pty pane has no provider");
});

test("returns null when agent_type is unknown and backing is pty", () => {
  seed();
  assert(
    resolvePaneProvider(pane({ agent_type: "mystery" })) === null,
    "unknown agent with no chat_backend backing resolves to null",
  );
});

test("returns null for a missing pane", () => {
  assert(resolvePaneProvider(null) === null, "null pane -> null");
  assert(resolvePaneProvider(undefined) === null, "undefined pane -> null");
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
console.log(`paneProvider: ${tests.length - failed}/${tests.length} passed`);
if (failed > 0) process.exit(1);
