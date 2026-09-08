import assert from "node:assert/strict";
import { finish, it, vi } from "../test/standaloneTest";

it("whole-peer Web terminals use tab-scoped gateway streams without cross-peer output", async () => {
  const values = new Map<string, string>();
  const storage = {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => values.set(key, value),
    removeItem: (key: string) => values.delete(key),
  };
  const browser = Object.assign(new EventTarget(), {
    location: { protocol: "http:", host: "gateway.test:7685" },
  });
  vi.stubGlobal("window", browser);
  vi.stubGlobal("localStorage", storage);
  vi.stubGlobal("sessionStorage", storage);
  const api = await import("./webApi");
  const output = await import("./outputStream");
  api.setTokenFor("http://gateway.test:7685", "opaque-local-token");
  const requests: URL[] = [];
  vi.stubGlobal("fetch", vi.fn(async (...args: unknown[]) => {
    const url = new URL(String(args[0]));
    requests.push(url);
    assert.equal(url.origin, "http://gateway.test:7685");
    assert.equal(url.searchParams.get("path"), "/api/output/stream");
    assert.equal((args[1] as RequestInit).headers &&
      new Headers((args[1] as RequestInit).headers).get("Authorization"), "Bearer opaque-local-token");
    return new Response(new ReadableStream({ start(controller) {
      const encode = (body: object) => new TextEncoder().encode(`data: ${JSON.stringify(body)}\n\n`);
      controller.enqueue(encode({ tab_id: "foreign-tab", output: "not-owned", offset: 99 }));
      controller.enqueue(encode({ tab_id: "same-tab", output: "owned", offset: 5, boot: "boot" }));
    } }));
  }));
  const unsubscribers: Array<() => void> = [];
  try {
    for (const [selected, tabId, expectedPeer] of [
      ["peer-a", "same-tab", "peer-a"],
      ["peer-b", "same-tab", "peer-b"],
      ["peer-b", "mesh:peer-c/same-tab", "peer-c"],
    ]) {
      api.setRelayTarget(selected, selected);
      const received: string[] = [];
      let resolve!: () => void;
      const delivered = new Promise<void>((done) => { resolve = done; });
      unsubscribers.push(output.subscribeTabOutput(tabId, {
        onOutput(value) { received.push(value); resolve(); }, onTruncated() {}, onEnded() {},
      }));
      assert.equal(requests[requests.length - 1]?.searchParams.get("tab"), `mesh:${expectedPeer}/same-tab`);
      assert.equal(requests[requests.length - 1]?.searchParams.get("peer"), expectedPeer);
      await delivered;
      assert.deepEqual(received, ["owned"]);
    }
  } finally {
    for (const unsubscribe of unsubscribers) unsubscribe();
    output.resetOutputStream();
    vi.unstubAllGlobals();
  }
});

finish();
