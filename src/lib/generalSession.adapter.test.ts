import type { HttpMethod } from "./transportClient";
import { describe, expect, it } from "vitest";
import { spawnFreshGeneralSession } from "./generalSession";
import { createTabsTransport, type TabsTransportClient } from "./transport/tabs";
import type { AtomicSpawn } from "./types";

const result: AtomicSpawn = {
  tab_id: "fresh", workspace_id: "general",
  snapshot: { workspaces: [], tabs: {}, active_workspace_id: "general", focused_tab_id: "main" },
};

describe("fresh General HTTP transport", () => {
  it("sends one authoritative General intent through the domain adapter", async () => {
    const calls: unknown[] = [];
    const client: TabsTransportClient = {
      async json<T>(method: HttpMethod, path: string, body?: unknown): Promise<T> {
        calls.push({ method, path, body });
        return result as T;
      },
    };
    const transport = createTabsTransport(client);
    const request = <T,>(command: string, args?: Record<string, unknown>): Promise<T> => {
      if (command !== "spawn_general_agent_session") throw new Error("Unexpected command");
      return transport.spawn_general_agent_session(args ?? {}) as Promise<T>;
    };
    expect(await spawnFreshGeneralSession("general", "configured-provider", "/instance/general", request,
      { model: "chosen-model", reasoningEffort: "high", preset: "demo" })).toEqual(result);
    expect(calls).toEqual([{
      method: "POST", path: "/api/agents/general/sessions",
      body: { workspaceId: "general", providerId: "configured-provider", cwd: "/instance/general",
        role: "general_agent", intent: "fresh_general", model: "chosen-model", reasoningEffort: "high", preset: "demo" },
    }]);
  });
});
