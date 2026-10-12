import { describe, expect, it, vi } from "vitest";
import { instrumentFetch, requestFailure } from "./requestDiagnostics";

const page = "http://127.0.0.1:1463/index.html";
const target = "http://127.0.0.1:7690/api/tabs?token=secret#secret";

describe("request diagnostics", () => {
  it("reports only request metadata and preserves network failures", async () => {
    const error = new TypeError("secret");
    const report = vi.fn();
    const wrapped = instrumentFetch(vi.fn().mockRejectedValue(error), () => page, report);
    await expect(wrapped(target, { method: "POST", headers: { Authorization: "Bearer secret" } })).rejects.toBe(error);
    expect(report).toHaveBeenCalledExactlyOnceWith({
      method: "POST", path: "/api/tabs", pageOrigin: "http://127.0.0.1:1463",
      targetOrigin: "http://127.0.0.1:7690", kind: "network-or-cors",
    });
    expect(JSON.stringify(report.mock.calls)).not.toContain("secret");
  });
  it("distinguishes HTTP, opaque, local network and abort outcomes", () => {
    expect(requestFailure(target, undefined, page, { type: "basic", ok: false, status: 403 })).toMatchObject({ kind: "http", status: 403 });
    expect(requestFailure(target, undefined, page, { type: "opaque", ok: false, status: 0 })).toMatchObject({ kind: "opaque" });
    expect(requestFailure("/api/tabs", undefined, page)).toMatchObject({ kind: "network" });
    expect(requestFailure(target, { signal: AbortSignal.abort() }, page)).toMatchObject({ kind: "aborted" });
  });
  it("preserves responses and prevents diagnostic recursion", async () => {
    const response = new Response("denied", { status: 401 });
    const report = vi.fn();
    const wrapped = instrumentFetch(vi.fn().mockResolvedValue(response), () => page, report);
    expect(await wrapped("http://127.0.0.1:7690/api/frontend/diagnostic")).toBe(response);
    expect(report).not.toHaveBeenCalled();
    expect(requestFailure(target, undefined, page, { type: "basic", ok: true, status: 200 })).toBeUndefined();
  });
});
