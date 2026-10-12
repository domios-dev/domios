// Run: npx tsx src/components/Sidebar/tabProviderMenu.test.ts
import assert from "node:assert/strict";
import { test } from "node:test";
import {
  switchTabProvider,
  tabProviderEntryLabel,
  tabProviderMenuView,
  type TabProviderSwitchApi,
} from "./tabProviderMenu";
import { __setInvokeForTest } from "../../lib/bridge";
import { getTabProviderApi } from "./tabProviderApi";
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
  const view = tabProviderMenuView(tab({ agent_session_id: "bound-session", agent_state: { state: "working" } }), CATALOG, "Working");
  assert.ok(view, "expected a switch surface");
  assert.ok(view.blockedReason, "a restart mid-turn must be refused");
  assert.match(view.blockedReason, /turn/i);
});

test("protected home tabs offer the same capability-based provider choice", () => {
  const view = tabProviderMenuView(tab({ protected: true }), CATALOG, "Idle");
  assert.ok(view);
  assert.equal(view.blockedReason, null);
  assert.deepEqual(view.providers.map((provider) => provider.id), ["claude", "codex"]);
});

test("switching requests one provider restart without changing identity first", async () => {
  const calls: Array<[string, string]> = [];
  const api: TabProviderSwitchApi = {
    restartWithProvider: async (tabId, providerId) => { calls.push([tabId, providerId]); },
  };
  await switchTabProvider(api, "t1", "codex");
  assert.deepEqual(calls, [["t1", "codex"]]);
});

test("a refused provider restart is surfaced without another operation", async () => {
  let calls = 0;
  const api: TabProviderSwitchApi = {
    restartWithProvider: async () => { calls++; throw new Error("refused"); },
  };
  await assert.rejects(switchTabProvider(api, "t1", "codex"), /refused/);
  assert.equal(calls, 1);
});

test("the default provider API requests the atomic lifecycle operation", async () => {
  const calls: Array<{ command: string; args: unknown }> = [];
  __setInvokeForTest(async <T>(command: string, args?: Record<string, unknown>) => {
    calls.push({ command, args });
    return undefined as T;
  });
  try {
    await switchTabProvider(getTabProviderApi(), "home", "opaque-provider");
    assert.deepEqual(calls, [{ command: "restart_agent_tab", args: {
      tabId: "home", providerId: "opaque-provider",
    } }]);
  } finally {
    __setInvokeForTest(null);
  }
});
