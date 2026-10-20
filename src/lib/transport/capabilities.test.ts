import { describe, expect, it } from "vitest";
import { createCapabilitiesTransport, invokeCapability, type CapabilitiesClient } from "./capabilities";

function fixture(result: unknown = {}) {
  const calls: unknown[][] = [];
  const request = async <T>(method: string, path: string, body?: unknown): Promise<T> => {
    calls.push([method, path, body]);
    return structuredClone(result) as T;
  };
  const client: CapabilitiesClient = {
    json: request,
    localJson: async <T>(method: string, path: string, body?: unknown) => {
      calls.push(["local", method, path, body]);
      return structuredClone(result) as T;
    },
    currentToken: () => "client-token",
  };
  return { calls, transport: createCapabilitiesTransport(client), client };
}

describe("capabilities HTTP transport", () => {
  it("has all 94 lane commands and rejects unregistered schema commands", async () => {
    const { transport, calls } = fixture();
    expect(Object.keys(transport)).toHaveLength(94);
    await expect(invokeCapability(transport, "arbitrary_rust_command")).rejects.toThrow("No audited HTTP route");
    await expect(invokeCapability(transport, "toString")).rejects.toThrow("No audited HTTP route");
    expect(calls).toEqual([]);
  });

  it("carries grouped model usage without changing it into quota", async () => {
    const groups = [{ group: "fast", messages: 3, input_tokens: 4, output_tokens: 5, cost_micros: 6 }];
    const { transport, calls } = fixture(groups);
    expect(await transport.provider_model_usage({ providerId: "drop/in" })).toEqual(groups);
    expect(calls).toEqual([["GET", "/api/agents/providers/drop%2Fin/model-usage", undefined]]);
  });

  it("requires explicit integration consent and preserves provider identity", async () => {
    const { transport, calls } = fixture();
    await expect(transport.install_provider_integration({ provider_id: "drop-in" })).rejects.toThrow("requires consent");
    await transport.install_provider_integration({ provider_id: "drop-in", consent: true });
    await transport.uninstall_provider_integration({ provider_id: "drop-in" });
    expect(calls).toEqual([
      ["POST", "/api/setup/hooks/install?provider=drop-in", { consent: true, provider: "drop-in" }],
      ["POST", "/api/setup/hooks/uninstall?provider=drop-in", { provider: "drop-in" }],
    ]);
  });

  it("forwards schema provider arguments through every integration and rule capability", async () => {
    const { transport, calls } = fixture();
    const args = { provider_id: "fixture/a & b", consent: true };
    for (const command of [
      "check_provider_integration", "install_provider_integration", "uninstall_provider_integration",
      "restore_provider_integration", "check_provider_rule", "install_provider_rule",
    ]) await invokeCapability(transport, command, args);
    expect(calls).toEqual([
      ["GET", "/api/setup/hooks?provider=fixture%2Fa%20%26%20b", undefined],
      ["POST", "/api/setup/hooks/install?provider=fixture%2Fa%20%26%20b", { consent: true, provider: "fixture/a & b" }],
      ["POST", "/api/setup/hooks/uninstall?provider=fixture%2Fa%20%26%20b", { provider: "fixture/a & b" }],
      ["POST", "/api/setup/hooks/restore?provider=fixture%2Fa%20%26%20b", undefined],
      ["GET", "/api/setup/rules?provider=fixture%2Fa%20%26%20b", undefined],
      ["POST", "/api/setup/rules/install?provider=fixture%2Fa%20%26%20b", undefined],
    ]);
  });

  it("uses PATCH for configuration and restores only the authenticated client's token", async () => {
    const { transport, calls } = fixture({ api: { token: "redacted", port: 7685 } });
    expect(await transport.get_config()).toEqual({ api: { token: "client-token", port: 7685 } });
    await transport.update_config({ config: { theme: { name: "custom" } } });
    expect(calls[1]).toEqual(["PATCH", "/api/config", { theme: { name: "custom" } }]);
  });

  it("uses host operations in either shell, with bare results preserved", async () => {
    const { transport, calls } = fixture("host-result");
    expect(await transport.get_default_cwd()).toBe("host-result");
    expect(await transport.get_themes_dir()).toBe("host-result");
    await transport.get_shell_diagnostics();
    await transport.suggest_install_pwsh();
    await transport.attachment_path_kinds({ paths: ["host/file"] });
    await transport.report_webgl_status({ state: "loaded" });
    expect(calls).toEqual([
      ["GET", "/api/system/default-cwd", undefined],
      ["GET", "/api/themes/location", undefined],
      ["GET", "/api/system/shell-diagnostics", undefined],
      ["POST", "/api/system/install-pwsh", undefined],
      ["POST", "/api/attachments/path-kinds", { paths: ["host/file"] }],
      ["POST", "/api/diagnostics/webgl", { state: "loaded", detail: undefined }],
    ]);
  });

  it("uploads bytes through attachment APIs and unwraps persisted paths", async () => {
    const { transport, calls } = fixture({ path: "/persisted/attachment" });
    expect(await transport.save_clipboard_image({ bytes: [1, 2], ext: "png" })).toBe("/persisted/attachment");
    expect(await transport.save_clipboard_file({ bytes: [3], filename: "file.txt" })).toBe("/persisted/attachment");
    expect(calls).toEqual([
      ["POST", "/api/attachments/clipboard-image", { bytes: [1, 2], ext: "png" }],
      ["POST", "/api/attachments/clipboard-file", { bytes: [3], filename: "file.txt" }],
    ]);
    expect(transport).not.toHaveProperty("copy_image_to_clipboard");
  });

  it("keeps local daemon attachment handoff and model provider operations local", async () => {
    const { transport, calls } = fixture();
    await transport.mesh_transfer_attachment({ ordinaryTabId: "mesh:peer/tab", peerId: "peer", path: "/file", kind: "image" });
    await transport.model_provider_manage({ providerId: "provider/a", request: { action: "list" } });
    expect(calls).toEqual([
      ["local", "POST", "/api/mesh/attachments/transfer", { ordinary_tab_id: "mesh:peer/tab", peer_id: "peer", path: "/file", kind: "image" }],
      ["local", "POST", "/api/model-providers?provider=provider%2Fa", { action: "list" }],
    ]);
  });

  it("preserves notification answer identity and rejects preview requests without a tab", async () => {
    const { transport, calls } = fixture();
    await transport.answer_notification({ id: "notice/a", label: "Approve" });
    expect(calls).toEqual([["POST", "/api/notifications/notice%2Fa/answer", { label: "Approve" }]]);
    await expect(transport.read_html_preview_file({ path: "/file" })).rejects.toThrow("tab identity");
  });

  it("propagates HTTP errors without dropping status or reason", async () => {
    const { client } = fixture();
    const failure = Object.assign(new Error("provider unavailable"), { status: 503 });
    client.json = async () => { throw failure; };
    await expect(createCapabilitiesTransport(client).provider_model_usage({ providerId: "p" })).rejects.toBe(failure);
  });
});
