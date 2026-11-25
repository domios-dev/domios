import { afterEach, describe, expect, it, vi } from "vitest";
import { __setInvokeForTest } from "./bridge";
import { probeProviderInstallMap, probeProviderInstalled } from "./providerInstallStatus";
import type { ProviderDescriptor } from "./types";

function descriptor(overrides: Partial<ProviderDescriptor>): ProviderDescriptor {
  return {
    schema_version: 1,
    id: "fixture",
    display_name: "Fixture",
    transport: "pty",
    system_prompt_delivery: { kind: "external" },
    settings_schema: [],
    notifications_schema: [],
    supports_lazy_skills: false,
    has_hooks: false,
    available_models: [],
    ...overrides,
  };
}

describe("probeProviderInstalled", () => {
  afterEach(() => {
    __setInvokeForTest(null);
  });

  it("probes single-binary agentRuntime providers instead of assuming chat_backend needs no CLI", async () => {
    const invoke = vi.fn(async (_cmd: string, args?: Record<string, unknown>) => ({
      required: true,
      installed: args?.providerId !== "missing-runtime",
      binary: "runtime-cli",
      resolved_path: null,
      availability: args?.providerId === "missing-runtime" ? "unavailable" : "available",
      searched: [],
      installer: "opaque-install",
      updater: null,
    }));
    __setInvokeForTest(invoke as unknown as Parameters<typeof __setInvokeForTest>[0]);

    const runtimeMissing = descriptor({
      id: "missing-runtime",
      transport: "chat_backend",
      cli_binary: "runtime-cli",
      agent_runtime: { runtime: "http-sse", command: { binary: "runtime-cli", args: [] } } as never,
    });
    const pureChatBackend = descriptor({
      id: "pure-chat",
      transport: "chat_backend",
    });
    const profiledRuntime = descriptor({
      id: "profiled",
      transport: "chat_backend",
      agent_runtime: { runtime: "stdio-jsonrpc", launchProfiles: { local: { binary: "local", args: [] } } } as never,
    });

    const map = await probeProviderInstallMap([runtimeMissing, pureChatBackend, profiledRuntime]);
    expect(map).toEqual({
      "missing-runtime": false,
      "pure-chat": true,
      "profiled": true,
    });
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith("provider_cli_status", { providerId: "missing-runtime" });

    await probeProviderInstalled(runtimeMissing, { refresh: true });
    expect(invoke).toHaveBeenLastCalledWith("provider_cli_status", { providerId: "missing-runtime", refresh: true });
  });
});
