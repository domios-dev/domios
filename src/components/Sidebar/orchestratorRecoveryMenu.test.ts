import assert from "node:assert/strict";
import type { ProviderDescriptor, Tab } from "../../lib/types";
import {
  orchestratorRecoveryMenuView,
  replaceOrchestrator,
  type OrchestratorRecoveryApi,
} from "./orchestratorRecoveryMenu";

function tab(overrides: Partial<Tab> = {}): Tab {
  return {
    id: "hubble",
    pty_id: "pty-hubble",
    backing: { kind: "pty" },
    tab_type: "Terminal",
    title: "Hubble",
    cwd: "D:/Developer/codeterm",
    agent_type: null,
    agent_session_id: "session-hubble",
    role: "Manager",
    role_profile: "orchestrator",
    team_id: "team-hubble",
    boot_stage: "shell_ready",
    agent_booting: false,
    ...overrides,
  };
}

function provider(id: string, transport: "pty" | "chat_backend"): ProviderDescriptor {
  return {
    schema_version: 1,
    id,
    display_name: id,
    transport,
    system_prompt_delivery: "external",
    available_models: [],
    settings_schema: [],
    notifications_schema: [],
    supports_lazy_skills: false,
    has_hooks: false,
  } as ProviderDescriptor;
}

const ptyProvider = provider("provider-neutral", "pty");
const chatProvider = provider("chat-neutral", "chat_backend");
const hubble = tab();
const eligible = orchestratorRecoveryMenuView(hubble, [chatProvider, ptyProvider], [hubble]);
if (!eligible) throw new Error("expected recovery menu eligibility");
assert.equal(eligible.teamId, "team-hubble");
assert.deepEqual(eligible.providers.map((candidate) => candidate.id), ["provider-neutral"]);

for (const ineligible of [
  tab({ agent_type: "provider-neutral", boot_stage: "agent_ready" }),
  tab({ agent_type: "provider-neutral", boot_stage: "pty_reattached" }),
  tab({ agent_booting: true }),
  tab({ role: null, role_profile: null }),
  tab({ role_profile: "planner" }),
  tab({ backing: { kind: "chat_backend", plugin_id: "chat-neutral", session_id: "chat-1" } }),
  tab({ pty_id: null }),
  tab({ team_id: null }),
  tab({ boot_stage: "spawn_failed" }),
]) {
  assert.equal(orchestratorRecoveryMenuView(ineligible, [ptyProvider], [ineligible]), null);
}
assert.equal(orchestratorRecoveryMenuView(hubble, [chatProvider], [hubble]), null);
assert.equal(orchestratorRecoveryMenuView(hubble, [ptyProvider], undefined), null);

for (const competitor of [
  tab({ id: "live-owner", agent_type: "provider-neutral", boot_stage: "agent_ready" }),
  tab({ id: "unobserved-owner", agent_type: "provider-neutral", boot_stage: "pty_reattached" }),
]) {
  assert.equal(
    orchestratorRecoveryMenuView(hubble, [ptyProvider], [hubble, competitor]),
    null,
  );
}
assert(orchestratorRecoveryMenuView(hubble, [ptyProvider], [hubble]));

const calls: Array<{ teamId: string; providerId: string }> = [];
const api: OrchestratorRecoveryApi = {
  replace: async (teamId, providerId) => {
    calls.push({ teamId, providerId });
  },
};

async function runReplacementAssertions(): Promise<void> {
  await replaceOrchestrator(eligible, "provider-neutral", api);
  assert.deepEqual(calls, [{ teamId: "team-hubble", providerId: "provider-neutral" }]);

  await assert.rejects(() => replaceOrchestrator(eligible, "chat-neutral", api));
  assert.equal(calls.length, 1);

  console.log("Orchestrator recovery menu tests: passed");
}

void runReplacementAssertions().catch((error: unknown) => {
  console.error(error);
  process.exitCode = 1;
});
