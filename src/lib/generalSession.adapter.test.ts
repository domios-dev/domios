import { afterEach, describe, expect, it, vi } from "vitest";
import type { AtomicSpawn } from "./types";

const result: AtomicSpawn = {
  tab_id: "fresh", workspace_id: "general",
  snapshot: { workspaces: [], tabs: {}, active_workspace_id: "general", focused_tab_id: "main" },
};

afterEach(() => {
  vi.resetModules();
  vi.doUnmock("./webApi");
  vi.doUnmock("@tauri-apps/api/core");
});

describe("fresh General transport adapters", () => {
  for (const web of [false, true]) {
    it(`sends one authoritative General intent through ${web ? "HTTP" : "desktop IPC"}`, async () => {
      vi.resetModules();
      const desktop = vi.fn(async () => result);
      const http = vi.fn(async () => result);
      vi.doMock("@tauri-apps/api/core", () => ({ invoke: desktop }));
      vi.doMock("./webApi", async (importOriginal) => ({
        ...await importOriginal<typeof import("./webApi")>(), IS_WEB: web, webJson: http,
      }));
      const { spawnFreshGeneralSession } = await import("./generalSession");
      const actual = await spawnFreshGeneralSession("general", "configured-provider", "/instance/general", undefined, { model: "chosen-model", reasoningEffort: "high", preset: "demo" });
      const params = {
        workspaceId: "general", providerId: "configured-provider", cwd: "/instance/general",
        role: "general_agent", intent: "fresh_general",
        model: "chosen-model", reasoningEffort: "high", preset: "demo",
      };
      expect(actual).toEqual(result);
      if (web) {
        expect(http).toHaveBeenCalledExactlyOnceWith("POST", "/api/agents/general/sessions", params);
        expect(desktop).not.toHaveBeenCalled();
      } else {
        expect(desktop).toHaveBeenCalledExactlyOnceWith("spawn_general_agent_session", { params });
        expect(http).not.toHaveBeenCalled();
      }
    });
  }
});
