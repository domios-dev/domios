import assert from "node:assert/strict";
import { Window } from "happy-dom";
import { requestTeamMember } from "./teamActions";

async function main() {
  const win = new Window({ url: "http://fixture-gateway:7686" });
  for (const [name, value] of Object.entries({ window: win, document: win.document, localStorage: win.localStorage, sessionStorage: win.sessionStorage })) {
    Object.defineProperty(globalThis, name, { configurable: true, value });
  }
  const { invoke } = await import("./bridge");
  const { setTokenFor, selfHost } = await import("./webApi");
  setTokenFor(selfHost(), "fixture-token");
  const previousFetch = globalThis.fetch;
  const calls: Array<{ url: string; init?: RequestInit }> = [];
  const response = { tab_id: "fixture-cartographer", team: { id: "fixture-team" }, reused: true };
  globalThis.fetch = async (input, init) => {
    calls.push({ url: String(input), init });
    return Response.json(response);
  };
  try {
    const brief = 'opaque-owner-data\n`$()`\n{"mode":"refresh"}';
    const result = await requestTeamMember(invoke, {
      teamId: "fixture-team", role: "cartographer", providerId: "fixture-provider",
      managerTabId: "fixture-manager", model: "fixture-model", brief, sessionBindingNonce: "fixture-nonce",
    });
    assert.deepEqual(result, response);
    assert.equal(calls.length, 1);
    assert.equal(new URL(calls[0].url).pathname, "/api/teams/fixture-team/members");
    assert.equal(calls[0].init?.method, "POST");
    assert.equal(new Headers(calls[0].init?.headers).get("X-CodeTerm-Tab-Id"), null);
    assert.deepEqual(JSON.parse(String(calls[0].init?.body)), {
      provider: "fixture-provider", role: "cartographer", model: "fixture-model", task: brief, caller_tab_id: "fixture-manager", session_binding_nonce: "fixture-nonce", sessionBindingNonce: "fixture-nonce",
    });
  } finally {
    globalThis.fetch = previousFetch;
  }
  console.log("Team briefing bridge checks passed");
}

void main();
