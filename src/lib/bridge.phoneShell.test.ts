import { afterEach, describe, expect, it, vi } from "vitest";

const native = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => native);

afterEach(() => {
  vi.unstubAllGlobals();
  vi.doUnmock("./webApi");
  vi.resetModules();
});

describe("phone shell transport contracts", () => {
  it("sends a JSON path sequence and preserves the returned map files", async () => {
    const files = [{ project_path: "/opaque/one", content: "opaque map" }];
    const fetch = vi.fn().mockResolvedValue(Response.json(files));
    vi.stubGlobal("fetch", fetch);
    const { invoke } = await import("./bridge");
    const paths = ["/opaque/one & two", "C:/opaque/three"];
    expect(await invoke("read_workspace_essence_maps", { projectPaths: paths })).toEqual(files);
    const [url, init] = fetch.mock.calls[0];
    expect(new URL(url).pathname).toBe("/api/essence-maps");
    expect(new URL(url).search).toBe("");
    expect(init.method).toBe("POST");
    expect(JSON.parse(init.body)).toEqual({ path: paths });
  });

  it("routes every integration re-check through the authenticated provider endpoint", async () => {
    const health = { integration_state: "installed", session_start_hook: true };
    const fetch = vi.fn().mockImplementation(async () => Response.json(health));
    vi.stubGlobal("fetch", fetch);
    const { checkProviderIntegration, webCapability } = await import("./bridge");
    const { setTokenFor, selfHost } = await import("./webApi");
    setTokenFor(selfHost(), "opaque-token");
    expect(webCapability("check_provider_integration")).toEqual({ supported: true });
    for (let attempt = 0; attempt < 2; attempt++) {
      expect(await checkProviderIntegration("opaque & provider")).toEqual(health);
    }
    expect(fetch).toHaveBeenCalledTimes(2);
    for (const [url, init] of fetch.mock.calls) {
      expect(new URL(url).pathname).toBe("/api/setup/hooks");
      expect(new URL(url).searchParams.get("provider")).toBe("opaque & provider");
      expect(init.method).toBe("GET");
      expect(new Headers(init.headers).get("Authorization")).toBe("Bearer opaque-token");
    }
  });

  it("keeps request diagnostics out of the map failure returned to the screen", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response("opaque private path", { status: 400 })));
    const { invoke } = await import("./bridge");
    const error = await invoke("read_workspace_essence_maps", { projectPaths: ["/opaque/private"] }).catch(value => value);
    expect(error).toBeInstanceOf(Error);
    if (!(error instanceof Error)) throw error;
    expect(error.message).not.toContain("opaque");
    expect(error.message).not.toContain("400");
    expect(error.message).not.toContain("/api/");
  });

  it("preserves desktop Tauri commands and arguments", async () => {
    vi.doMock("./webApi", async () => ({ ...await vi.importActual<object>("./webApi"), IS_WEB: false }));
    const { invoke, checkProviderIntegration } = await import("./bridge");
    native.invoke.mockResolvedValue({ opaque: true });
    await invoke("read_workspace_essence_maps", { projectPaths: ["/opaque/project"] });
    expect(native.invoke).toHaveBeenLastCalledWith("read_workspace_essence_maps", { projectPaths: ["/opaque/project"] });
    expect(await checkProviderIntegration("opaque-provider")).toEqual({ opaque: true });
    expect(native.invoke).toHaveBeenLastCalledWith("check_provider_integration", { providerId: "opaque-provider" });
  });
});
