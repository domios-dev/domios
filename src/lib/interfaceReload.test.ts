import { describe, expect, it, vi } from "vitest";
import { interfaceReloadUrl, prepareInterfaceReload } from "./interfaceReload";

class Worker extends EventTarget {
  state: ServiceWorkerState = "installed";
  private requested!: () => void;
  readonly request = new Promise<void>(resolve => { this.requested = resolve; });
  postMessage = vi.fn(() => { this.requested(); });
  change(state: ServiceWorkerState) { this.state = state; this.dispatchEvent(new Event("statechange")); }
}

function fixture(worker: Worker | null) {
  const registration = { waiting: worker, installing: null, update: vi.fn(async () => {}) };
  const container = { getRegistration: vi.fn(async () => registration) } as unknown as Pick<ServiceWorkerContainer, "getRegistration">;
  return { registration, container };
}

describe("interface reload", () => {
  it("preserves the current route, query and auth fragment while bypassing the old index cache key", () => {
    const url = new URL(interfaceReloadUrl("https://fixture.test/chat?tab=owned#token=opaque", "next-build"));
    expect(url.pathname).toBe("/chat");
    expect(url.searchParams.get("tab")).toBe("owned");
    expect(url.searchParams.get("__ct_ui")).toBe("next-build");
    expect(url.hash).toBe("#token=opaque");
    expect(interfaceReloadUrl(url.href, "third")).not.toContain("next-build");
  });
  it("waits for activation after requesting the waiting worker", async () => {
    const worker = new Worker(), { container, registration } = fixture(worker);
    const ready = vi.fn();
    const task = prepareInterfaceReload(container, new AbortController().signal).then(ready);
    await worker.request;
    expect(registration.update).toHaveBeenCalledOnce();
    expect(ready).not.toHaveBeenCalled();
    worker.change("activating");
    expect(ready).not.toHaveBeenCalled();
    worker.change("activated");
    await task;
    expect(ready).toHaveBeenCalledOnce();
  });
  it("arms activation observation before a synchronous worker acknowledgement", async () => {
    const worker = new Worker(), { container } = fixture(worker);
    worker.postMessage.mockImplementation(() => worker.change("activated"));
    await prepareInterfaceReload(container, new AbortController().signal);
    expect(worker.postMessage).toHaveBeenCalledOnce();
  });
  it("rejects a cancelled activation instead of treating the deadline as readiness", async () => {
    const worker = new Worker(), { container } = fixture(worker), abort = new AbortController();
    const task = prepareInterfaceReload(container, abort.signal);
    await worker.request;
    const failure = new Error("deadline");
    abort.abort(failure);
    await expect(task).rejects.toBe(failure);
  });
  it("allows a page without a service worker", async () => {
    await prepareInterfaceReload(undefined, new AbortController().signal);
  });
});
