import { afterEach, describe, expect, it, vi } from "vitest";
import { selectedDaemonTransport } from "./daemonRuntime";
import { launchProviderUpdate } from "./providerUpdate";

afterEach(() => {
  vi.restoreAllMocks();
});

describe("launchProviderUpdate", () => {
  it("relays composite remote tab updates to that exact peer with its own tab id", async () => {
    const localJson = vi.spyOn(selectedDaemonTransport, "localJson").mockResolvedValue({
      ok: true,
      data: { tab_id: "cli-update-1", launch_state: "command_dispatched" },
      peer: { instance_id: "blackstar" },
    });
    const json = vi.spyOn(selectedDaemonTransport, "json");

    const launch = await launchProviderUpdate("claude", "mesh:blackstar/remote-tab-7");
    expect(launch).toEqual({ tab_id: "cli-update-1", launch_state: "command_dispatched" });
    expect(json).not.toHaveBeenCalled();
    expect(localJson).toHaveBeenCalledExactlyOnceWith("POST", "/api/mesh/relay-to", {
      target_instance_id: "blackstar",
      method: "POST",
      path: "/api/agents/providers/claude/cli",
      body: { operation: "update", caller_tab_id: "remote-tab-7" },
    });
  });

  it("relays Settings updates to the explicitly viewed remote peer without a caller tab id", async () => {
    const localJson = vi.spyOn(selectedDaemonTransport, "localJson").mockResolvedValue({
      ok: true,
      data: { tab_id: "cli-update-2", launch_state: "command_dispatched" },
      peer: { instance_id: "blackstar" },
    });

    await launchProviderUpdate("codex", undefined, "blackstar");
    expect(localJson).toHaveBeenCalledExactlyOnceWith("POST", "/api/mesh/relay-to", {
      target_instance_id: "blackstar",
      method: "POST",
      path: "/api/agents/providers/codex/cli",
      body: { operation: "update" },
    });
  });

  it("propagates remote refusals and rejects mismatched answering peers", async () => {
    vi.spyOn(selectedDaemonTransport, "localJson").mockResolvedValueOnce({
      ok: false,
      error: "HTTP 400: No supported CLI update target is available",
      peer: { instance_id: "blackstar" },
    });
    await expect(launchProviderUpdate("claude", "mesh:blackstar/tab-1"))
      .rejects.toThrow("No supported CLI update target is available");

    vi.spyOn(selectedDaemonTransport, "localJson").mockResolvedValueOnce({
      ok: true,
      data: { tab_id: "cli-update-3", launch_state: "command_dispatched" },
      peer: { instance_id: "wrong-peer" },
    });
    await expect(launchProviderUpdate("claude", "mesh:blackstar/tab-1"))
      .rejects.toThrow("different installation");
  });

  it("refuses malformed composite tab ids without touching any daemon", async () => {
    const localJson = vi.spyOn(selectedDaemonTransport, "localJson");
    const json = vi.spyOn(selectedDaemonTransport, "json");
    await expect(launchProviderUpdate("claude", "mesh:broken")).rejects.toThrow("local fallback is forbidden");
    expect(localJson).not.toHaveBeenCalled();
    expect(json).not.toHaveBeenCalled();
  });

  it("targets the local transport directly for local tabs", async () => {
    const json = vi.spyOn(selectedDaemonTransport, "json").mockResolvedValue({
      tab_id: "local-cli-tab",
      launch_state: "command_dispatched",
    });
    const localJson = vi.spyOn(selectedDaemonTransport, "localJson");

    const launch = await launchProviderUpdate("claude", "local-tab-1");
    expect(launch.tab_id).toBe("local-cli-tab");
    expect(localJson).not.toHaveBeenCalled();
    expect(json).toHaveBeenCalledExactlyOnceWith("POST", "/api/agents/providers/claude/cli", {
      operation: "update",
      caller_tab_id: "local-tab-1",
    });
  });
});
