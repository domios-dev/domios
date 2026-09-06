// The provider indicator (TabItem header + TabSessionChat footer) resolves via
// useTabProvider -> resolveTabProvider(tab). This proves a backend

import { strict as assert } from "node:assert";
import { reducer } from "./reducer";
import type { ExtendedState } from "./tree";
import type { MultiplexerState, Tab, Workspace, ProviderDescriptor } from "../../lib/types";
import { setAgentDescriptors, getAgentDescriptor } from "../../lib/agentDescriptorCache";
import { resolveTabProvider } from "../../lib/tabProvider";

let failures = 0;
function test(name: string, fn: () => void) {
  try {
    fn();
    console.log(`  ✓ ${name}`);
  } catch (e) {
    failures++;
    console.error(`  ✗ ${name}: ${e instanceof Error ? e.message : String(e)}`);
  }
}

function base(): ExtendedState {
  return {
    workspaces: [],
    tabs: {},
    teams: [],
    active_workspace_id: null,
    focused_tab_id: null,
    notifications: [],
    tabStatuses: {},
    toasts: [],
    activeTabIds: {},
    generation: 0,
    pendingCloses: [],
  };
}

function descriptor(id: string, display: string, color: string): ProviderDescriptor {
  return {
    id,
    display_name: display,
    transport: "pty",
    icon_color: color,
    icon_hint: display.slice(0, 2),
  } as unknown as ProviderDescriptor;
}

function snapWithProvider(generation: number, agentType: string | null): MultiplexerState {
  return {
    workspaces: [
      { id: "w1", name: "W", tabs: ["ga"], active_tab_id: "ga", cwd: null } as unknown as Workspace,
    ],
    tabs: {
      ga: { id: "ga", pty_id: "pty-1", title: "Main", agent_type: agentType } as unknown as Tab,
    },
    active_workspace_id: "w1",
    focused_tab_id: "ga",
    generation,
  };
}

// Both descriptors are live in the cache (the real steady state after a switch:
// backend detection resolves correctly, Cmd+R already proved it). The defect was
setAgentDescriptors([
  descriptor("codex", "Codex", "#58a6ff"),
  descriptor("claude", "Claude Code", "#d97757"),
]);

test("indicator re-resolves to the new provider after a switch state-changed event", () => {
  const asCodex = reducer(base(), { type: "SET_STATE", state: snapWithProvider(1, "codex") });
  const before = resolveTabProvider(asCodex.tabs.ga);
  assert(before?.id === "codex" && before.displayName === "Codex", "starts resolved to Codex");

  // Same store, same indicator — only the backend snapshot changed provider.
  const asClaude = reducer(asCodex, { type: "SET_STATE", state: snapWithProvider(2, "claude") });
  const after = resolveTabProvider(asClaude.tabs.ga);
  assert(after?.id === "claude", "indicator re-resolves to the Claude descriptor id");
  assert(after?.displayName === "Claude Code", "footer/header name flips to Claude Code");
  assert(after?.iconColor === "#d97757", "logo (icon) re-resolves to the Claude icon");
  // Invalidation, not a fresh-mount coincidence: the SAME tab record the store
  // produced now yields Claude where it previously yielded Codex.
  assert.notEqual(before?.id, after?.id, "the stale Codex resolution was invalidated");
});

test("unrelated state-changed does not thrash the resolved provider", () => {
  const asClaude = reducer(base(), { type: "SET_STATE", state: snapWithProvider(1, "claude") });
  const first = resolveTabProvider(asClaude.tabs.ga);
  const unrelated = reducer(asClaude, { type: "SET_STATE", state: snapWithProvider(2, "claude") });
  const second = resolveTabProvider(unrelated.tabs.ga);
  assert(first?.id === "claude" && second?.id === "claude", "provider stays Claude");
  assert(first?.displayName === second?.displayName, "no needless re-resolution churn");
  assert(getAgentDescriptor("claude")?.display_name === "Claude Code", "descriptor cache intact");
});

console.log(failures === 0 ? "✓ provider indicator re-resolve tests green" : `✗ ${failures} failure(s)`);
process.exit(failures === 0 ? 0 : 1);
