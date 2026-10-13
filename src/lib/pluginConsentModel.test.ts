import { describe, expect, it, vi } from "vitest";
import { installConsentRequests, missingPermissions, requiredGrant, runWithConsent, type ConsentPlugin } from "./pluginConsentModel";

const plugin: ConsentPlugin = { id: "fixture", display_name: "Fixture", sensitive_permissions: [
  { key: "local_recording", label: "Record your selected microphone", allowed: false },
  { key: "spawn_agents", label: "Start agents", allowed: true, decided: true },
] };

describe("plugin consent decisions", () => {
  it("only prompts declared missing grants and new installation decisions", () => {
    expect(missingPermissions(plugin).map((item) => item.permission)).toEqual(["local_recording"]);
    expect(installConsentRequests(plugin).map((item) => item.permission)).toEqual(["local_recording"]);
    expect(installConsentRequests({ ...plugin, sensitive_permissions: [{ key: "local_recording", label: "Record", allowed: false, decided: true }] })).toEqual([]);
    expect(requiredGrant("spawn_agents_grant_required", plugin)?.permission).toBe("spawn_agents");
    expect(requiredGrant("unknown_grant_required", plugin)).toBeNull();
    expect(requiredGrant("xlocal_recording_grant_required", plugin)).toBeNull();
  });
  it("recognizes rejected, returned and declarative permission failures", () => {
    for (const error of [new Error("Error: local_recording_grant_required"), { error: "local_recording_grant_required" }, { nodes: [{ kind: "text", text: "local_recording_grant_required" }] }]) {
      expect(requiredGrant(error, plugin)?.permission).toBe("local_recording");
    }
  });
  it("allows extra exact network hosts only for declared network access", () => {
    expect(requiredGrant("fetch denied: api.test:8443", plugin)).toBeNull();
    expect(requiredGrant("fetch denied: api.test:8443", { ...plugin, network_requested: true })?.permission).toBe("network:api.test:8443");
    expect(requiredGrant("fetch denied: api.test:443", { ...plugin, network_requested: true })?.permission).toBe("network:api.test:443");
  });
  it("records Allow before retrying the exact operation once", async () => {
    const order: string[] = [];
    const operation = vi.fn().mockImplementationOnce(async () => { order.push("attempt"); throw new Error("local_recording_grant_required"); })
      .mockImplementationOnce(async () => { order.push("retry"); return "enabled"; });
    const result = await runWithConsent(operation, plugin, async () => { order.push("consent"); return true; }, async () => { order.push("persist"); });
    expect(result).toBe("enabled");
    expect(order).toEqual(["attempt", "consent", "persist", "retry"]);
  });
  it("Not now never grants or retries and hides the raw code", async () => {
    const operation = vi.fn().mockRejectedValue(new Error("local_recording_grant_required"));
    const grant = vi.fn();
    await expect(runWithConsent(operation, plugin, async () => false, grant)).rejects.toThrow("Permission was not allowed.");
    expect(operation).toHaveBeenCalledTimes(1);
    expect(grant).not.toHaveBeenCalled();
  });
  it("a repeated refusal is bounded to one retry and readable", async () => {
    const operation = vi.fn().mockResolvedValue({ error: "local_recording_grant_required" });
    await expect(runWithConsent(operation, plugin, async () => true, async () => {})).rejects.toThrow("Permission is still unavailable.");
    expect(operation).toHaveBeenCalledTimes(2);
  });
  it("ordinary failures and successful actions never request consent", async () => {
    const consent = vi.fn();
    await expect(runWithConsent(async () => { throw new Error("offline"); }, plugin, consent, vi.fn())).rejects.toThrow("offline");
    expect(await runWithConsent(async () => 42, plugin, consent, vi.fn())).toBe(42);
    expect(consent).not.toHaveBeenCalled();
  });
  it("undeclared grant failures stay readable and never gain access", async () => {
    const consent = vi.fn();
    await expect(runWithConsent(async () => ({ error: "unknown_grant_required" }), plugin, consent, vi.fn())).rejects.toThrow("This plugin permission is unavailable.");
    expect(consent).not.toHaveBeenCalled();
  });
  it("a persistence failure never retries the action", async () => {
    const operation = vi.fn().mockRejectedValue(new Error("local_recording_grant_required"));
    await expect(runWithConsent(operation, plugin, async () => true, async () => { throw new Error("disk failure"); })).rejects.toThrow("disk failure");
    expect(operation).toHaveBeenCalledTimes(1);
  });
});
