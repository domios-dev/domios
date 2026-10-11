import { afterEach, describe, expect, it, vi } from "vitest";
import { createTransportClient } from "./transportClient";

function pending<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}

afterEach(() => { vi.unstubAllGlobals(); });

describe("aggregate terminal attachment", () => {
  it("replays a new terminal and resumes a retained terminal without duplicate bytes or PTY mutations", async () => {
    vi.resetModules();
    sessionStorage.setItem("codeterm-web-output-boot", "boot");
    localStorage.clear();
    const requests: Array<{ url: URL; signal: AbortSignal }> = [];
    let connection = pending<ReadableStreamDefaultController<Uint8Array>>();
    vi.stubGlobal("fetch", async (input: string, init: RequestInit) => {
      const signal = init.signal as AbortSignal;
      requests.push({ url: new URL(input), signal });
      expect(new Headers(init.headers).get("Authorization")).toBe("Bearer test-token");
      expect(new Headers(init.headers).get("X-CodeTerm-Session-Binding-Nonce")).toBe("exact-nonce");
      return new Response(new ReadableStream<Uint8Array>({
        start(controller) {
          signal.addEventListener("abort", () => controller.error(new DOMException("Detached", "AbortError")), { once: true });
          connection.resolve(controller);
        },
      }));
    });
    const output = await import("./outputStream");
    output.bindOutputStreamTransport(createTransportClient({ origin: "http://daemon.test", token: "test-token", headers: { "X-CodeTerm-Session-Binding-Nonce": "exact-nonce" } }));
    const desktop = {}, browser = {};
    const bytes: number[][] = [[], []];
    let written = pending<void>();
    const sink = (index: number) => ({
      onOutput() { throw new Error("Raw bytes required"); },
      onBytes(chunk: Uint8Array) { bytes[index].push(...chunk); written.resolve(); },
      onTruncated() {}, onEnded() {},
    });
    let detach = output.subscribeTabOutput("tab", sink(0), desktop, "pty");
    let detachBrowser = () => {};
    try {
      let controller = await connection.promise;
      const send = (target: ReadableStreamDefaultController<Uint8Array>, data: number[], offset: number) => {
        target.enqueue(new TextEncoder().encode(`data: ${JSON.stringify({ tab_id: "tab", pty_id: "pty", boot: "boot", data, output: "display", offset })}\n\n`));
      };
      send(controller, [0x1b, 0xff, 0xe2], 3);
      await written.promise;
      written = pending<void>();
      send(controller, [0x1b, 0xff, 0xe2], 3);
      send(controller, [0xe2, 0x82, 0xac], 5);
      await written.promise;
      expect(bytes[0]).toEqual([0x1b, 0xff, 0xe2, 0x82, 0xac]);
      detach();
      expect(requests[0].signal.aborted).toBe(true);
      connection = pending();
      detach = output.subscribeTabOutput("tab", sink(0), desktop, "pty");
      controller = await connection.promise;
      expect(JSON.parse(atob(requests[1].url.searchParams.get("since")!)).tabs).toEqual({ tab: 5 });
      written = pending<void>();
      send(controller, [0x1b, 0xff, 0xe2, 0x82, 0xac], 5);
      send(controller, [6], 6);
      await written.promise;
      expect(bytes[0]).toEqual([0x1b, 0xff, 0xe2, 0x82, 0xac, 6]);
      connection = pending();
      detachBrowser = output.subscribeTabOutput("tab", sink(1), browser, "pty");
      controller = await connection.promise;
      expect(JSON.parse(atob(requests[2].url.searchParams.get("since")!)).tabs).toEqual({ tab: 0 });
      written = pending<void>();
      send(controller, [0x1b, 0xff, 0xe2, 0x82, 0xac, 6], 6);
      await written.promise;
      expect(bytes).toEqual([
        [0x1b, 0xff, 0xe2, 0x82, 0xac, 6], [0x1b, 0xff, 0xe2, 0x82, 0xac, 6],
      ]);
      expect(requests.every(({ url }) => url.pathname === "/api/output/stream")).toBe(true);
    } finally {
      detachBrowser(); detach(); output.resetOutputStream();
    }
  });
});
