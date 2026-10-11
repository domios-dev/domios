import { configureDaemonTransport } from "./daemonBootstrap";
import { afterEach, describe, expect, finish, it, vi } from "../test/standaloneTest";
import { withFailureDeadline } from "../test/withFailureDeadline";
import { agentProvidersFromResponse, syncAgentDescriptorCache } from "../hooks/useAgentProviders";
import { getAgentDescriptor } from "./agentDescriptorCache";
import type { ProviderDescriptor } from "./types";
import { webJson, webUpload, invalidateWebRequestContext } from "./webApi";

describe("web JSON prompt response transport", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("resolves a vanished prompt as a quiet no-op", async () => {
    vi.stubGlobal("window", {
      location: { protocol: "http:", host: "localhost" },
      dispatchEvent: () => true,
      CustomEvent: class {},
    });
    vi.stubGlobal("localStorage", { getItem: () => null });
    configureDaemonTransport({ origin: `http://${window.location.host}`, token: "fixture-token" });
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
    configureDaemonTransport({ origin: `http://${window.location.host}`, token: "fixture-token" });
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
    configureDaemonTransport({ origin: `http://${window.location.host}`, token: "fixture-token" });
    vi.stubGlobal("fetch", async () => new Response(
      JSON.stringify({ ok: true, reason: "prompt_not_on_screen" }),
      { status: 409, headers: { "Content-Type": "application/json" } },
    ));

    await expect(webJson("POST", "/api/tabs/tab/prompt/respond", { value: "1" })).rejects.toThrow("HTTP 409");
  });
});

describe("web config response context", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("refuses a local response body completing after peer invalidation", async () => {
    vi.stubGlobal("window", {
      location: { protocol: "http:", host: "localhost" },
      dispatchEvent: () => true,
      CustomEvent: class {},
    });
    vi.stubGlobal("localStorage", { getItem: () => null });
    configureDaemonTransport({ origin: `http://${window.location.host}`, token: "fixture-token" });
    let beginBody!: () => void;
    const bodyStarted = new Promise<void>((resolve) => { beginBody = resolve; });
    let finishBody!: (value: string) => void;
    const body = new Promise<string>((resolve) => { finishBody = resolve; });
    vi.stubGlobal("fetch", async () => {
      const response = new Response();
      response.text = () => { beginBody(); return body; };
      return response;
    });
    const result = webJson("GET", "/api/config");
    const refused = expect(result).rejects.toThrow("Peer context changed");
    try {
      await withFailureDeadline(bodyStarted, "local config response body");
      invalidateWebRequestContext();
      finishBody(JSON.stringify({ mesh: { instance_name: "opaque-a" } }));
      await withFailureDeadline(refused, "stale body refusal");
    } finally { finishBody("{}"); }
  });
});

describe("peer provider catalog relay", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("routes to the owning peer and unwraps its descriptor catalog once", async () => {
    vi.stubGlobal("window", {
      location: { protocol: "http:", host: "gateway:7686" },
      dispatchEvent: () => true,
      CustomEvent: class {},
    });
    vi.stubGlobal("localStorage", {
      getItem: (key: string) => key.includes("relay-peer:") ? "owning-peer" : null,
    });
    configureDaemonTransport({ origin: `http://${window.location.host}`, token: "fixture-token" });
    const descriptor = { id: "peer-only-provider", supports_chat_ui: true } as ProviderDescriptor;
    vi.stubGlobal("fetch", async (url: string, init: RequestInit) => {
      expect(url).toBe("http://gateway:7686/api/mesh/relay-to");
      expect(JSON.parse(String(init.body))).toEqual({
        target_peer_id: "owning-peer", method: "GET", path: "/api/agents/providers", body: null,
      });
      return new Response(JSON.stringify({ ok: true, status: 200, data: [descriptor], error: null }));
    });
    const catalog = agentProvidersFromResponse(await webJson("GET", "/api/agents/providers"));
    syncAgentDescriptorCache(catalog, false, null);
    expect(getAgentDescriptor(descriptor.id)?.supports_chat_ui).toBe(true);
  });
});

describe("browser attachment upload transport", () => {
  afterEach(() => vi.unstubAllGlobals());
  it("uses browser multipart and interrupts a suspended voice upload", async () => {
    const page = new EventTarget();
    const doc = new EventTarget();
    vi.stubGlobal("window", Object.assign(page, { location: { protocol: "https:", host: "phone" } }));
    vi.stubGlobal("document", Object.assign(doc, { visibilityState: "visible" }));
    vi.stubGlobal("localStorage", { getItem: () => null });
    configureDaemonTransport({ origin: `http://${window.location.host}`, token: "fixture-token" });
    let started!: () => void;
    const requestStarted = new Promise<void>((resolve) => { started = resolve; });
    vi.stubGlobal("fetch", async (url: string, init: RequestInit) => {
      expect(url).toBe("https://phone/api/upload");
      expect(new Headers(init.headers).has("Content-Type")).toBe(false);
      const field = (init.body as FormData).get("file") as File;
      expect(field.name).toBe("voice-note.webm");
      expect(field.type).toBe("audio/webm");
      expect(await field.text()).toBe("voice");
      return new Promise<Response>((_resolve, reject) => {
        init.signal!.addEventListener("abort", () => reject(new DOMException("Interrupted", "AbortError")), { once: true });
        started();
      });
    });
    const result = webUpload(new Blob(["voice"], { type: "audio/webm" }), "voice-note.webm");
    const rejected = expect(result).rejects.toThrow("Interrupted");
    await withFailureDeadline(requestStarted, "voice upload start");
    page.dispatchEvent(new Event("pagehide"));
    await rejected;
  });
});

finish();
