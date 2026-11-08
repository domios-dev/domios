// Run: npx tsx src/lib/tabProvider.test.ts
// resolveTabProvider unifies tab identity across the two sources the UI has:

import { setAgentDescriptors } from "./agentDescriptorCache";
import { setPluginProviderMeta } from "./pluginProviderMeta";
import { resolveTabProvider, resolveAgentIcon } from "./tabProvider";
import type { Tab, ProviderDescriptor } from "./types";

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
};

function seed() {
  setAgentDescriptors([claudeDescriptor]);
  setPluginProviderMeta([
    {
      id: "lmstudio",
      displayName: "LM Studio",
      iconSvg: "<svg id='lmstudio'/>",
    },
  ]);
}

function tab(overrides: Partial<Tab>): Tab {
  return {
    id: "p1",
    pty_id: null,
    tab_type: "Terminal",
    title: "t",
    cwd: null,
    backing: { kind: "pty" },
    ...overrides,
  };
}

test("resolves an agent Tab from the descriptor cache", () => {
  seed();
  const r = resolveTabProvider(tab({ agent_type: "claude" }));
  assert(r, "expected a resolved provider");
  assert(r.kind === "agent", `expected agent kind, got ${r.kind}`);
  assert(r.id === "claude", `expected id claude, got ${r.id}`);
  assert(r.displayName === "Claude", `expected Claude, got ${r.displayName}`);
  assert(r.iconSvg === "<svg id='claude'/>", "agent icon svg from descriptor");
  assert(r.iconColor === "#cc785c", "agent icon color from descriptor");
  assert(r.iconHint === "C", "agent icon hint from descriptor");
});

test("resolves a shell Tab from the plugin snapshot cache", () => {
  seed();
  const r = resolveTabProvider(
    tab({ backing: { kind: "chat_backend", plugin_id: "lmstudio", session_id: "s1" } }),
  );
  assert(r, "expected a resolved provider");
  assert(r.kind === "shell", `expected shell kind, got ${r.kind}`);
  assert(r.id === "lmstudio", `expected id lmstudio, got ${r.id}`);
  assert(r.displayName === "LM Studio", `expected LM Studio, got ${r.displayName}`);
  assert(r.iconSvg === "<svg id='lmstudio'/>", "shell icon svg from plugin snapshot");
});

test("shell Tab still resolves as shell before its meta is cached", () => {
  setAgentDescriptors([claudeDescriptor]);
  setPluginProviderMeta([]); // snapshot not loaded yet
  const r = resolveTabProvider(
    tab({ backing: { kind: "chat_backend", plugin_id: "lmstudio", session_id: "s1" } }),
  );
  assert(r, "expected a resolved provider even without cached meta");
  assert(r.kind === "shell", `expected shell kind, got ${r.kind}`);
  assert(r.id === "lmstudio", "id falls back to plugin_id");
  assert(r.displayName === "lmstudio", "displayName falls back to plugin_id");
  assert(!r.iconSvg, "no icon svg until snapshot loads");
});

test("returns null for a plain terminal Tab", () => {
  seed();
  assert(resolveTabProvider(tab({})) === null, "plain pty Tab has no provider");
});

test("agent Tab resolves before descriptor cache loads", () => {
  seed();
  setAgentDescriptors([]);
  const r = resolveTabProvider(tab({ agent_type: "codex" }));
  assert(r, "expected fallback provider");
  assert(r.kind === "agent", `expected agent kind, got ${r.kind}`);
  assert(r.id === "codex", `expected codex, got ${r.id}`);
  assert(r.displayName === "Codex", `expected Codex, got ${r.displayName}`);
  assert(r.iconHint === "</>", "codex fallback icon is immediate");
});

test("unknown agent still resolves as an agent identity", () => {
  seed();
  const r = resolveTabProvider(tab({ agent_type: "mystery" }));
  assert(r, "expected unknown agent fallback");
  assert(r.kind === "agent", `expected agent kind, got ${r.kind}`);
  assert(r.id === "mystery", `expected mystery, got ${r.id}`);
  assert(r.iconHint === "MY", "unknown agent fallback uses id initials");
});

test("returns null for a missing Tab", () => {
  assert(resolveTabProvider(null) === null, "null Tab -> null");
  assert(resolveTabProvider(undefined) === null, "undefined Tab -> null");
});

test("icon precedence uses the profile, registry and generic fallback", () => {
  assert(resolveAgentIcon("profile", "registry", "generic") === "profile", "profile wins");
  assert(resolveAgentIcon(undefined, "registry", "generic") === "registry", "registry next");
  assert(resolveAgentIcon("", undefined, "generic") === "generic", "failure uses generic");
});

test("runtime identity and selected profile reach tab presentation without provider branches", () => {
  setAgentDescriptors([{...claudeDescriptor, id:"acp", icon_svg:"<svg id='generic'/>", agent_runtime:{launchProfiles:{"gemini-cli":{binary:"gemini",args:[],iconSvg:"<svg id='gemini'/>"}}}} as ProviderDescriptor]);
  const r = resolveTabProvider(tab({agent_type:"acp", agent_runtime_config:{launchProfile:"gemini-cli"}, agent_runtime_state:{state:"loaded",epoch:1,agentName:"Gemini CLI",binaryVersion:"1.2.3",iconSvg:"<svg id='registry'/>"}}));
  assert(r?.iconSvg === "<svg id='gemini'/>", "selected profile wired");
  assert(r.displayName === "Gemini CLI · 1.2.3", "own runtime name and version wired");
});

test("restored tabs resolve only their own profile icon and never the shared provider logo", () => {
  const profiles = {
    first: { binary: "first-agent", args: [], registryId: "first-registry", iconSvg: "<svg id='first'/>" },
    second: { binary: "second-agent", args: [], registryId: "second-registry", iconSvg: "<svg id='second'/>" },
  };
  const descriptor = { ...claudeDescriptor, id: "shared-runtime", icon_svg: "<svg id='shared-other'/>", agent_runtime: { launchProfiles: profiles } };
  setAgentDescriptors([descriptor]);
  const restored = (profile: string) => tab({ agent_type: descriptor.id, agent_runtime_config: { launchProfile: profile }, agent_runtime_state: { state: "loaded", epoch: 1, iconSvg: "" } });
  for (const profile of ["second", "first", "second"]) {
    const resolved = resolveTabProvider(restored(profile));
    assert(resolved?.iconSvg === `<svg id='${profile}'/>`, "each restored tab gets its own declared icon");
    assert(resolved.fallbackIconSvg === undefined, "shared provider is not a profile fallback");
  }
  setAgentDescriptors([{ ...descriptor, agent_runtime: { launchProfiles: { ...profiles, second: { ...profiles.second, iconSvg: undefined } } } }]);
  assert(resolveTabProvider(restored("second"))?.iconSvg === undefined, "missing own icon uses generic glyph, not the other profile or provider");
  assert(resolveTabProvider(restored("first"))?.iconSvg === profiles.first.iconSvg, "another profile remains unchanged");
  assert(resolveTabProvider(restored("unknown"))?.iconSvg === undefined, "unknown profile cannot borrow a logo");
});

test("sleeping profile tabs select declared icons without runtime sessions", () => {
  setAgentDescriptors([{ ...claudeDescriptor, id: "shared-runtime", icon_svg: "<svg id='shared'/>", agent_runtime: { launchProfiles: {
    declared: { binary: "declared-agent", args: [], iconSvg: "<svg id='sleeping-profile'/>" },
    unmarked: { binary: "unmarked-agent", args: [] },
  } } }]);
  const sleeping = (profile: string) => tab({ agent_type: "shared-runtime", agent_runtime_config: { launchProfile: profile }, slept_at_ms: 1, agent_runtime_state: null });
  assert(resolveTabProvider(sleeping("declared"))?.iconSvg === "<svg id='sleeping-profile'/>", "sleeping tab needs only its own descriptor icon");
  assert(resolveTabProvider(sleeping("unmarked"))?.iconSvg === undefined, "unmarked sleeping profile stays generic");
  assert(resolveTabProvider(sleeping("unmarked"))?.fallbackIconSvg === undefined, "no shared provider fallback while sleeping");
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
console.log(`tabProvider: ${tests.length - failed}/${tests.length} passed`);
if (failed > 0) process.exit(1);
