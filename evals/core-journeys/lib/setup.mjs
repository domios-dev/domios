import { dcJson } from "./env.mjs";

const firstTab = (value) => (Array.isArray(value) ? value[0] : value?.tabs?.[0] ?? value);

export function promoteFixtureOrchestrator(id, command = dcJson) {
  return command(["agent", "set-orchestrator", "--tab", id], { asTab: id });
}

export async function chooseAgentAccess(api) {
  let config = await api("/api/config");
  if (typeof config.agent?.full_access !== "boolean") {
    await api("/api/config", { method: "PATCH", body: { key: "agent.full_access", value: true } });
    config = await api("/api/config");
  }
  if (typeof config.agent?.full_access !== "boolean") throw new Error("Agent access choice was not persisted");
  return { full_access: config.agent.full_access };
}

export async function completeFirstRun(api, providerId) {
  let config = await api("/api/config");
  if (config.general_agent?.first_run_complete !== true) {
    if (!providerId) throw new Error("First run requires an installed provider");
    await api("/api/agents/general/complete-first-run", { method: "POST",
      body: { provider_id: providerId, full_access: config.agent.full_access } });
    config = await api("/api/config");
  }
  if (config.general_agent?.first_run_complete !== true) throw new Error("General Agent first run was not persisted");
  return { provider: config.general_agent.agent_provider, first_run_complete: true };
}

export function requireCdp(health) {
  const raw = health.instance?.cdp_port;
  const port = typeof raw === "string" && /^[0-9]+$/.test(raw) ? Number(raw) : raw;
  if (!Number.isInteger(port) || port < 1 || port > 65535) {
    throw new Error("Core journeys require an open named DEV window with cdp_port in /api/health. Launch it with CODETERM_DEV_INSTANCE, CODETERM_DEV_API_PORT and CODETERM_DEV_CDP_PORT through ./build.sh dev --wait (dev-run skill) before running the bench.");
  }
  return port;
}

export async function bindFixtureSession(ctx, id, session, provider, inspect = async () =>
  firstTab(await dcJson(["tab", "inspect", "--tab", id]))) {
  if (!provider) throw new Error("Fixture session requires its provider owner");
  const tab = await inspect();
  const nonce = tab?.session_binding_nonce;
  if (!nonce) throw new Error(`tab ${id} has no session binding nonce`);
  const response = await ctx.api(`/api/tabs/${id}/hook/session_start`, { method: "POST",
    body: { session_id: session, provider_id: provider, binding_nonce: nonce, source: "core-journeys" } });
  if (response?.ok !== true || response.deferred) throw new Error(`session_start not committed: ${JSON.stringify(response)}`);
  const bound = await inspect();
  if (bound?.agent_type !== provider || bound.agent_session_id !== session
    || bound.session_binding_nonce !== nonce || bound.reported_session_binding_nonce !== nonce) {
    throw new Error(`Fixture session ownership was not committed for tab ${id}`);
  }
}
