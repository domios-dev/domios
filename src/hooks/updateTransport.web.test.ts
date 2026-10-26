import assert from "node:assert/strict";
import { finish, it, vi } from "../test/standaloneTest";

it("web update channel reads and writes use the browser gateway and selected relay", async () => {
  const values = new Map<string, string>();
  const storage = {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => values.set(key, value),
    removeItem: (key: string) => values.delete(key),
  };
  vi.stubGlobal("window", Object.assign(new EventTarget(), {
    location: { protocol: "http:", host: "100.64.10.2:7697" }, CustomEvent,
  }));
  vi.stubGlobal("localStorage", storage);
  vi.stubGlobal("sessionStorage", storage);
  const api = await import("../lib/webApi");
  const { requestUpdateChannel } = await import("./updateTransport");
  const requests: Array<{ url: URL; init: RequestInit }> = [];
  const origin = "http://100.64.10.2:7697";
  const config = { host: "127.0.0.1", port: 7685, token: "desktop-only-token" };
  api.setTokenFor(origin, "gateway-token");
  vi.stubGlobal("fetch", async (input: unknown, init: RequestInit = {}) => {
    const url = new URL(String(input));
    requests.push({ url, init });
    assert.equal(url.origin, origin);
    assert.equal(new Headers(init.headers).get("Authorization"), "Bearer gateway-token");
    return Response.json(url.pathname === "/api/mesh/relay-to"
      ? { ok: true, status: 200, data: { channel: "rc" } } : { channel: "stable" });
  });
  try {
    await requestUpdateChannel(config);
    await requestUpdateChannel(config, "nightly");
    assert.equal(requests[0].url.pathname, "/api/update/channel");
    assert.equal(requests[0].init.method ?? "GET", "GET");
    assert.equal(requests[1].init.method, "POST");
    assert.deepEqual(JSON.parse(String(requests[1].init.body)), { channel: "nightly" });
    const peerId = "928b06dd-0631-43e2-9252-3c6573b54b8d";
    api.setRelayTarget(peerId, "Windows");
    await requestUpdateChannel(config);
    await requestUpdateChannel(config, "rc");
    for (const [request, method, body] of [
      [requests[2], "GET", null], [requests[3], "POST", { channel: "rc" }],
    ] as const) {
      assert.equal(request.url.pathname, "/api/mesh/relay-to");
      assert.deepEqual(JSON.parse(String(request.init.body)), {
        target_peer_id: peerId, method, path: "/api/update/channel", body,
      });
    }
  } finally {
    api.clearRelayTarget();
    vi.unstubAllGlobals();
  }
});

finish();
