import { afterEach, describe, expect, finish, it, vi } from "../test/standaloneTest";
import { webJson } from "./webApi";

describe("web JSON prompt response transport", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("resolves a vanished prompt as a quiet no-op", async () => {
    vi.stubGlobal("window", {
      location: { protocol: "http:", host: "localhost" },
      dispatchEvent: () => true,
      CustomEvent: class {},
    });
    vi.stubGlobal("localStorage", { getItem: () => null });
    vi.stubGlobal("fetch", async () => new Response(
      JSON.stringify({ ok: false, reason: "prompt_not_on_screen" }),
      { status: 409, headers: { "Content-Type": "application/json" } },
    ));

    const result = await webJson("POST", "/api/tabs/tab/prompt/respond", { value: "1" });
    expect(result).toEqual({ ok: false, reason: "prompt_not_on_screen" });
  });

  it("keeps other conflicts as errors", async () => {
    vi.stubGlobal("window", {
      location: { protocol: "http:", host: "localhost" },
      dispatchEvent: () => true,
      CustomEvent: class {},
    });
    vi.stubGlobal("localStorage", { getItem: () => null });
    vi.stubGlobal("fetch", async () => new Response(
      JSON.stringify({ reason: "other_conflict" }),
      { status: 409, headers: { "Content-Type": "application/json" } },
    ));

    await expect(webJson("POST", "/api/tabs/tab/prompt/respond", { value: "1" })).rejects.toThrow("HTTP 409");
  });

  it("requires the no-prompt response to be explicitly unsuccessful", async () => {
    vi.stubGlobal("window", {
      location: { protocol: "http:", host: "localhost" },
      dispatchEvent: () => true,
      CustomEvent: class {},
    });
    vi.stubGlobal("localStorage", { getItem: () => null });
    vi.stubGlobal("fetch", async () => new Response(
      JSON.stringify({ ok: true, reason: "prompt_not_on_screen" }),
      { status: 409, headers: { "Content-Type": "application/json" } },
    ));

    await expect(webJson("POST", "/api/tabs/tab/prompt/respond", { value: "1" })).rejects.toThrow("HTTP 409");
  });
});

finish();
