import assert from "node:assert/strict";
import { requestTeamMember } from "./teamActions";
import { createTransportClient } from "./transportClient";
import { bindOrchestrationTransport, orchestrationTransport } from "./transport/orchestrationRuntime";

async function main() {
  const calls: Array<{ url: string; init?: RequestInit }> = [];
  const response = { tab_id: "fixture-cartographer", team: { id: "fixture-team" }, reused: true };
  bindOrchestrationTransport(createTransportClient({
    origin: "http://fixture-gateway:7686", token: "fixture-token",
    headers: { "X-CodeTerm-Tab-Id": "fixture-manager", "X-CodeTerm-Binding-Nonce": "fixture-nonce" },
    fetch: async (input, init) => {
      calls.push({ url: String(input), init });
      return Response.json(response);
    },
  }));
  const brief = 'opaque-owner-data\n`$()`\n{"mode":"refresh"}';
  const result = await requestTeamMember(async (command, args) => {
    assert.equal(command, "mux_add_team_member");
    return orchestrationTransport().mux_add_team_member(args as unknown as Parameters<ReturnType<typeof orchestrationTransport>["mux_add_team_member"]>[0]);
  }, {
    teamId: "fixture-team", role: "cartographer", providerId: "fixture-provider",
    managerTabId: "fixture-manager", sessionBindingNonce: "fixture-nonce", model: "fixture-model", brief,
  });
  assert.deepEqual(result, response);
  assert.equal(calls.length, 1);
  assert.equal(new URL(calls[0].url).pathname, "/api/teams/fixture-team/members");
  assert.equal(calls[0].init?.method, "POST");
  const headers = new Headers(calls[0].init?.headers);
  assert.equal(headers.get("Authorization"), "Bearer fixture-token");
  assert.equal(headers.get("X-CodeTerm-Tab-Id"), "fixture-manager");
  assert.equal(headers.get("X-CodeTerm-Binding-Nonce"), "fixture-nonce");
  assert.deepEqual(JSON.parse(String(calls[0].init?.body)), {
    provider: "fixture-provider", role: "cartographer", model: "fixture-model", task: brief,
    caller_tab_id: "fixture-manager", session_binding_nonce: "fixture-nonce",
  });
  console.log("Team briefing HTTP adapter checks passed");
}

void main();
