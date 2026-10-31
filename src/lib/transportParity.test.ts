import { describe, expect, it } from "vitest";
import { scanTransportSource } from "./transportParityScan";

describe("transportParity", () => {
  it("detects aliases, dynamic commands, reexports and dynamic Tauri imports", () => {
    const source = `import { invoke as call } from './bridge';
      call('get_config'); call(schema.command);
      export { getCurrentWindow } from '@tauri-apps/api/window';
      const native = import('@tauri-apps/api/core');`;
    expect(scanTransportSource("fixture.ts", source).map(({ kind, target }) => ({ kind, target }))).toEqual([
      { kind: "audited-command", target: "get_config" }, { kind: "audited-command", target: "<dynamic>" },
      { kind: "tauri-import", target: "@tauri-apps/api/window" }, { kind: "tauri-import", target: "@tauri-apps/api/core" },
    ]);
  });
  it("ignores comments, string examples and test runtime marker creation", () => {
    expect(scanTransportSource("fixture.ts", `// import '@tauri-apps/api/core';\nconst example = "invoke('get_config')"; window.__TAURI_INTERNALS__ = {};`)).toEqual([]);
  });
  it("detects raw native globals and CommonJS imports", () => {
    const source = `window.__TAURI_INTERNALS__.invoke('get_config'); window['__TAURI__'].core.invoke('get_config'); require('@tauri-apps/api/core');`;
    expect(scanTransportSource("fixture.ts", source).map(site => site.kind)).toEqual(["native-global", "native-global", "tauri-import"]);
  });

});

import { createCommandRegistry, dispatchCommand, CLIENT_LOCAL_COMMANDS } from "./transportCommands";
import { rustRouteContracts, routeMatches } from "./transportRouteAudit";
import type { TransportClient } from "./transportClient";
import fixtures from "./transportCommandContracts.json";

const result = { ok: true, status: 200, data: [], paired: [], plans: [], themes: [], path: "path", text: "text", output: "output", line: "line", offset: 4, pty_id: "pty", generation: 1,
  tabs: {tab:{agent_session_id:"root",session_binding_nonce:"nonce",agent_runtime_state:{state:"loaded",epoch:4}}} };
function fixture() {
  const calls: string[][] = [];
  const json = async <T>(method: string, path: string): Promise<T> => { calls.push([method, path]); return structuredClone(result) as T; };
  const client: TransportClient = { json, localJson: json, upload: async () => { throw new Error("Unexpected upload"); }, subscribe: () => () => {}, currentToken: () => "token", origin: () => "http://127.0.0.1:7686" };
  return { client, calls };
}

describe("audited command registry", () => {
  it("executes every named adapter with its pinned HTTP method and path", async () => {
    const { client, calls } = fixture();
    const commands = createCommandRegistry(client);
    expect(Object.keys(commands).filter(name => !CLIENT_LOCAL_COMMANDS.includes(name as typeof CLIENT_LOCAL_COMMANDS[number])).sort()).toEqual(Object.keys(fixtures.commands).filter(name => name !== "kill_pty").sort());
    for (const [command, expected] of Object.entries(fixtures.commands)) {
      calls.length = 0;
      await dispatchCommand(commands, command, fixtures.args);
      expect(calls, command).toEqual(expected);
    }
    console.info("[transportParity mode=enforce] zero pending HTTP contracts");
  });
  it("keeps explicit peer commands on the base client when a relay is selected", async () => {
    const { client } = fixture();
    const localCalls: string[] = [];
    client.json = async () => { throw new Error("Unexpected selected-peer request"); };
    client.localJson = async <T>(_method: string, path: string) => {
      localCalls.push(path);
      return { ok: true, data: [], paired: [] } as T;
    };
    const registry = createCommandRegistry(client);
    await dispatchCommand(registry, "mesh_peer_tabs_list", { peerId: "peer" });
    await dispatchCommand(registry, "mesh_remote_tab_action", { peerId: "peer", ordinaryTabId: "mesh:peer/tab", suffix: "/inspect", method: "GET" });
    expect(localCalls).toEqual(["/api/mesh/relay-to", "/api/mesh/peers", "/api/mesh/relay-to"]);
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
