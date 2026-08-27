// Run: npx tsx src/components/Sidebar/tabProviderMenu.test.ts
import assert from "node:assert/strict";
import { test } from "node:test";
import {
  switchTabProvider,
  tabProviderEntryLabel,
  tabProviderMenuView,
  type TabProviderSwitchApi,
} from "./tabProviderMenu";
import type { ProviderDescriptor, Tab } from "../../lib/types";

function descriptor(id: string, transport: "pty" | "chat_backend"): ProviderDescriptor {
  return {
    schema_version: 1,
    id,
    display_name: id.toUpperCase(),
    transport,
    system_prompt_delivery: "argv",
    available_models: [],
  } as unknown as ProviderDescriptor;
}

function tab(over: Partial<Tab> = {}): Tab {
  return { id: "t1", agent_type: "claude", ...over } as unknown as Tab;
}

const CATALOG = [descriptor("claude", "pty"), descriptor("codex", "pty"), descriptor("lmstudio", "chat_backend")];

test("a PTY agent tab is offered the other PTY providers", () => {
  const view = tabProviderMenuView(tab(), CATALOG, "Idle");
  assert.ok(view, "expected a switch surface");
  assert.deepEqual(view.providers.map((p) => p.id), ["claude", "codex"], "chat backends are not PTY alternatives");
  assert.equal(view.currentProviderId, "claude");
  assert.equal(view.blockedReason, null);
});

test("the entry names the provider it is on", () => {
  const view = tabProviderMenuView(tab(), CATALOG, "Idle");
  assert.equal(tabProviderEntryLabel(view!), "Provider: CLAUDE");
});

test("a chat-backend tab has no PTY provider switch", () => {
  const view = tabProviderMenuView(
    tab({ backing: { kind: "chat_backend" } as Tab["backing"] }),
    CATALOG,
    "Idle",
  );
  assert.equal(view, null, "a plugin transport owns its own session");
});

test("a tab with no agent has nothing to switch", () => {
  assert.equal(tabProviderMenuView(tab({ agent_type: null }), CATALOG, "Idle"), null);
});

test("a lone PTY provider is not worth a picker", () => {
  assert.equal(tabProviderMenuView(tab(), [descriptor("claude", "pty")], "Idle"), null);
});

test("a working turn blocks the switch and says why", () => {
  const view = tabProviderMenuView(tab(), CATALOG, "Working");
  assert.ok(view, "expected a switch surface");
  assert.ok(view.blockedReason, "a restart mid-turn must be refused");
  assert.match(view.blockedReason, /turn/i);
});

test("switching sets the type and restarts, in that order", async () => {
  const calls: string[] = [];
  const api: TabProviderSwitchApi = {
    setAgentType: (tabId, agentType) => { calls.push(`set:${tabId}:${agentType}`); },
    restartSession: async (tabId) => { calls.push(`restart:${tabId}`); },
  };
  await switchTabProvider(api, "t1", "codex");
  assert.deepEqual(calls, ["set:t1:codex", "restart:t1"], "metadata alone leaves the old agent running");
});

test("a failed type write does not restart the tab", async () => {
  const calls: string[] = [];
  const api: TabProviderSwitchApi = {
    setAgentType: () => { throw new Error("refused"); },
    restartSession: async (tabId) => { calls.push(`restart:${tabId}`); },
  };
  await assert.rejects(switchTabProvider(api, "t1", "codex"));
  assert.deepEqual(calls, [], "restarting after a refused switch would relaunch the old provider");
});
