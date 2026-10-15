import { readFileSync, readdirSync } from "node:fs";
import { join, relative } from "node:path";
import { describe, expect, it } from "vitest";
import { scanTransportSource } from "./transportParityScan";

function sourceFiles(directory: string): string[] {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    if (["node_modules", "dist", "target", "generated"].includes(entry.name)) return [];
    const path = join(directory, entry.name);
    if (entry.isDirectory()) return sourceFiles(path);
    return /\.[cm]?[jt]sx?$/.test(entry.name) && !/\.(test|spec|testdata)\./.test(entry.name) ? [path] : [];
  });
}

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
  it("enforces zero native bypasses in src and packages without an allowlist", () => {
    const { client } = fixture();
    const known = new Set([...Object.keys(createCommandRegistry(client)), ...Object.keys(NATIVE_COMMANDS), ...Object.keys(CANNED_COMMANDS)]);
    const sites = ["src", "packages"].flatMap(sourceFiles).flatMap((path) => {
      const file = relative(process.cwd(), path).replaceAll("\\", "/");
      return file === "src/lib/nativeBridge.ts" ? [] : scanTransportSource(file, readFileSync(path, "utf8"));
    });
    const bypasses = sites.filter(site => site.kind !== "audited-command");
    expect(bypasses).toEqual([]);
    const unknown = sites.filter(site => site.kind === "audited-command" && site.target !== "<dynamic>" && !known.has(site.target));
    expect(unknown).toEqual([]);
    console.info(`[transportParity mode=enforce] ${bypasses.length} non-native bypasses; ${unknown.length} unknown named commands`);
    // Parses every source file in src and packages; a light CI runner needs about 7 s.
  }, 60_000);
  it("detects raw native globals and CommonJS imports", () => {
    const source = `window.__TAURI_INTERNALS__.invoke('get_config'); window['__TAURI__'].core.invoke('get_config'); require('@tauri-apps/api/core');`;
    expect(scanTransportSource("fixture.ts", source).map(site => site.kind)).toEqual(["native-global", "native-global", "tauri-import"]);
  });

});

import { createCommandRegistry, dispatchCommand, CLIENT_LOCAL_COMMANDS, CANNED_COMMANDS } from "./transportCommands";
import { rustRouteContracts, routeMatches } from "./transportRouteAudit";
import type { TransportClient } from "./transportClient";
import fixtures from "./transportCommandContracts.json";
import { NATIVE_COMMANDS } from "./nativeCommands";

function registeredContracts() {
  const routes = readFileSync("src-tauri/crates/codeterm-service-core/src/api/routes.rs", "utf8");
  const modules = readFileSync("src-tauri/crates/codeterm-service-core/src/api/mod.rs", "utf8");
  const sources = [routes];
  for (const domain of ["transport_tabs", "transport_capabilities", "transport_orchestration", "frontend_shell_routes"]) {
    expect(modules).toContain(`pub mod ${domain};`);
    expect(routes.split(`${domain}::router()`).length - 1, `${domain} registered exactly once`).toBe(1);
    {
      const sourcePath = domain === "frontend_shell_routes" ? `${domain}.rs` : `${domain}/mod.rs`;
      sources.push(readFileSync(`src-tauri/crates/codeterm-service-core/src/api/${sourcePath}`, "utf8"));
    }
  }
  expect(routes).toContain("tab_agent_routes::mount(router)");
  sources.push(readFileSync("src-tauri/crates/codeterm-service-core/src/api/tab_agent_routes.rs", "utf8"));
  return sources.flatMap(rustRouteContracts);
}

const result = { ok: true, status: 200, data: [], paired: [], plans: [], themes: [], path: "path", text: "text", output: "output", line: "line", offset: 4, pty_id: "pty", generation: 1 };
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
    const registered = registeredContracts();
    for (const [command, expected] of Object.entries(fixtures.commands)) {
      calls.length = 0;
      await dispatchCommand(commands, command, fixtures.args);
      expect(calls, command).toEqual(expected);
      for (const [method, path] of calls) {
        expect(registered.some(route => routeMatches(route, method, path)), `${command}: ${method} ${path} has no registered route`).toBe(true);
      }
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
  it("covers native Settings schema commands with audited routes", async () => {
    const source = readFileSync("src-tauri/crates/codeterm-plugin/src/agent_claude/provider/schemas.rs", "utf8");
    const names = [...source.matchAll(/(?:query_command|command):\s*"([a-z_]+)"/g)].map(match => match[1]);
    const { client, calls } = fixture();
    const registry = createCommandRegistry(client);
    for (const name of names) {
      if (Object.prototype.hasOwnProperty.call(registry, name)) expect(Object.prototype.hasOwnProperty.call(fixtures.commands, name)).toBe(true);
      else throw new Error(`Missing schema route: ${name}`);
    }
    for (const name of ["toString", "constructor", "__proto__", "arbitrary_rust_command"]) await expect(dispatchCommand(registry, name)).rejects.toThrow("No audited HTTP route");
    expect(calls).toEqual([]);
    expect(CANNED_COMMANDS).toEqual({ check_cli_installed: true });
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
