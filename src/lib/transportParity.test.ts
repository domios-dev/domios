import { describe, expect, it } from "vitest";

import { createCommandRegistry, dispatchCommand, CLIENT_LOCAL_COMMANDS } from "./transportCommands";
import { rustRouteContracts, routeMatches } from "./transportRouteAudit";
import type { TransportClient } from "./transportClient";
import fixtures from "./transportCommandContracts.json";

const peerScopedCommands = [
  "usage_history", "usage_latest", "usage_projection", "provider_model_usage",
  "provider_cli_status", "install_provider_in_tab",
] as const;
const localScopedCommands = new Set<string>([...peerScopedCommands, "usage_refresh"]);

const result = { ok: true, status: 200, data: [], peer: { instance_id: "peer", instance_name: "Peer" }, paired: [], plans: [], themes: [], path: "path", text: "text", output: "output", line: "line", offset: 4, pty_id: "pty", generation: 1,
  tabs: {tab:{agent_session_id:"root",session_binding_nonce:"nonce",agent_runtime_state:{state:"loaded",epoch:4}}} };
function reply(path: string, body?: unknown) {
  if (path === "/api/mesh/relay-to" && (body as { path?: string } | undefined)?.path === "/api/mux/state") {
    return { ...result, data: { tabs: {} } };
  }
  return structuredClone(result);
}
function fixture() {
  const calls: string[][] = [];
  const json = async <T>(method: string, path: string, body?: unknown): Promise<T> => { calls.push([method, path]); return reply(path, body) as T; };
  const client: TransportClient = { json, localJson: json, upload: async () => { throw new Error("Unexpected upload"); }, subscribe: () => () => {}, currentToken: () => "token", origin: () => "http://127.0.0.1:7686" };
  return { client, calls };
}

describe("audited command registry", () => {
  it("executes every named adapter with its pinned HTTP method and path", async () => {
    const { client, calls } = fixture();
    const commands = createCommandRegistry(client);
    expect(Object.keys(commands).filter(name => !CLIENT_LOCAL_COMMANDS.includes(name as typeof CLIENT_LOCAL_COMMANDS[number])).sort()).toEqual(Object.keys(fixtures.commands).filter(name => name !== "kill_pty").sort());
    const { peerId, ...localArgs } = fixtures.args;
    const localSpawns = new Set(["spawn_general_agent_session", "mux_spawn_tab"]);
    for (const [command, expected] of Object.entries(fixtures.commands)) {
      calls.length = 0;
      const args = localSpawns.has(command) ? localArgs : { ...localArgs, peerId: localScopedCommands.has(command) ? null : peerId };
      await dispatchCommand(commands, command, args);
      expect.soft(calls, command).toEqual(expected);
    }
    console.info("[transportParity mode=enforce] zero pending HTTP contracts");
  });
  it("pins usage reads to the local owner or exact peer and refreshes on the owning machine", async () => {
    const { client } = fixture();
    const localCalls: Array<[string, string, unknown]> = [];
    client.json = async () => { throw new Error("Unexpected selected-peer request"); };
    client.localJson = async <T>(method: string, path: string, body?: unknown) => {
      localCalls.push([method, path, body]);
      return reply(path, body) as T;
    };
    const commands = createCommandRegistry(client);
    for (const command of peerScopedCommands) {
      const [[method, path]] = fixtures.commands[command];
      localCalls.length = 0;
      await dispatchCommand(commands, command, { ...fixtures.args, peerId: null });
      await dispatchCommand(commands, command, fixtures.args);
      expect(localCalls, command).toEqual([
        [method, path, undefined],
        ["POST", "/api/mesh/relay-to", { target_instance_id: "peer", method, path, body: null }],
      ]);
    }
    localCalls.length = 0;
    await dispatchCommand(commands, "usage_refresh", { ...fixtures.args, peerId: null });
    await expect(dispatchCommand(commands, "usage_refresh", fixtures.args)).rejects.toThrow("owning machine");
    expect(localCalls).toEqual([["POST", "/api/usage/refresh", { provider: "provider", account_id: "account" }]]);
  });
  it("keeps explicit peer commands on the base client when a relay is selected", async () => {
    const { client } = fixture();
    const localCalls: string[] = [];
    client.json = async () => { throw new Error("Unexpected selected-peer request"); };
    client.localJson = async <T>(_method: string, path: string, body?: unknown) => {
      localCalls.push(path);
      return reply(path, body) as T;
    };
    const registry = createCommandRegistry(client);
    await dispatchCommand(registry, "mesh_peer_tabs_list", { peerId: "peer" });
    await dispatchCommand(registry, "mesh_remote_tab_action", { peerId: "peer", ordinaryTabId: "mesh:peer/tab", suffix: "/inspect", method: "GET" });
    expect(localCalls).toEqual(["/api/mesh/relay-to", "/api/mesh/relay-to"]);
  });
  it("rejects unknown and prototype command names before dispatch", async () => {
    const { client, calls } = fixture();
    const registry = createCommandRegistry(client);
    for (const name of ["toString", "constructor", "__proto__", "arbitrary_rust_command"]) await expect(dispatchCommand(registry, name)).rejects.toThrow("No audited HTTP route");
    expect(calls).toEqual([]);
  });
  it("keeps chat-view state and shared-PTY detach explicitly client-local", async () => {
    const { client, calls } = fixture();
    const registry = createCommandRegistry(client);
    await dispatchCommand(registry, "mux_set_chat_view", { tabId: "local-tab", chatView: true });
    expect(await dispatchCommand(registry, "kill_pty", { ptyId: "shared-pty" })).toBeNull();
    expect(calls).toEqual([]);
  });
  it("pins route methods and does not accept a mapper for a different verb", () => {
    const routes = rustRouteContracts('.route("/api/config", get(read).patch(update))');
    expect(routes.map(route => route.method)).toEqual(["GET", "PATCH"]);
    expect(routes.some(route => routeMatches(route, "POST", "/api/config"))).toBe(false);
  });
});

it("audits local-owner POST wrappers as the same concrete POST route",()=>{
  expect(rustRouteContracts('router.route("/api/plugins/view-call", plugin_host_action_routes::owner_post(handler))'))
    .toEqual([{method:"POST",path:"/api/plugins/view-call"}]);
});
