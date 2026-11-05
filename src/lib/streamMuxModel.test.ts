import { describe, expect, it } from "vitest";
import { BULK_CREDIT_STEP, bulkCreditDue, decodeMuxFrame, isDirectDaemonPath, isMuxStreamPath, muxSessionRefused } from "./streamMuxModel";

describe("mux stream routes", () => {
  it("shares events, startup, output, relay, agent-run and surface streams", () => {
    for (const path of ["/api/events/stream?since=b:4", "/api/output/stream", "/api/mesh/relay-stream?peer=p", "/api/tabs/t/output/stream?from=3", "/api/surfaces/t/stream"]) expect(isMuxStreamPath(path)).toBe(true);
    // Startup readiness must not depend on the carrier that itself waits for startup.
    expect(isMuxStreamPath("/api/startup/events")).toBe(false);
    expect(isDirectDaemonPath("/api/startup/events")).toBe(true);
    expect(isDirectDaemonPath("/api/events/stream")).toBe(false);
    for (const path of ["/api/stream-mux?session=s", "//evil/api/output/stream", "/api\\output/stream", "/api/tabs/t/chat-send"]) expect(isMuxStreamPath(path)).toBe(false);
  });
});

describe("mux response codec", () => {
  it("carries HTTP status, filtered headers, UTF-8 and binary bytes, and bounded endings", () => {
    expect(decodeMuxFrame({ channel: "c", open: true, status: 503, headers: [["content-type", "application/json"]] }))
      .toEqual({ channel: "c", kind: "open", status: 503, headers: [["content-type", "application/json"]] });
    expect(decodeMuxFrame({ channel: "c", chunk: "雪", enc: "utf8" })).toEqual({ channel: "c", kind: "chunk", chunk: "雪", enc: "utf8", credit: 0 });
    expect(decodeMuxFrame({ channel: "c", chunk: "/w==", enc: "base64" })).toEqual({ channel: "c", kind: "chunk", chunk: "/w==", enc: "base64", credit: 0 });
    expect(decodeMuxFrame({ channel: "c", chunk: "/w==", enc: "base64", credit: 8192 })).toMatchObject({ kind: "chunk", credit: 8192 });
    for (const bad of [-1, 1.5, "8", true]) expect(decodeMuxFrame({ channel: "c", chunk: "/w==", enc: "base64", credit: bad })).toMatchObject({ credit: 0 });
    expect(decodeMuxFrame({ channel: "c", end: { reason: "capacity", bytes: 64 } })).toEqual({ channel: "c", kind: "end", end: { reason: "capacity", bytes: 64 } });
  });

  it("refuses malformed frames and the removed data-frame protocol", () => {
    for (const invalid of [
      null, "x", {}, { channel: "" }, { channel: "c" }, { channel: "c", data: "legacy" },
      { channel: "c", open: true, status: 199, headers: [] }, { channel: "c", open: true, status: 200, headers: [["bad"]] },
      { channel: "c", chunk: "x", enc: "other" }, { channel: "c", end: { reason: "http", status: 404 } },
    ]) expect(decodeMuxFrame(invalid)).toBeNull();
  });
});

describe("mux flow control and refusal", () => {
  it("posts a cumulative bulk credit once every step and never before", () => {
    expect(bulkCreditDue(BULK_CREDIT_STEP - 1, 0)).toBeNull();
    expect(bulkCreditDue(BULK_CREDIT_STEP, 0)).toBe(BULK_CREDIT_STEP);
    expect(bulkCreditDue(BULK_CREDIT_STEP * 2 + 1, BULK_CREDIT_STEP)).toBe(BULK_CREDIT_STEP * 2 + 1);
    expect(bulkCreditDue(BULK_CREDIT_STEP, BULK_CREDIT_STEP)).toBeNull();
  });

  it("treats only credential refusals of the session as final", () => {
    for (const status of [401, 403]) expect(muxSessionRefused(status)).toBe(true);
    for (const status of [undefined, 0, 404, 410, 429, 500, 503]) expect(muxSessionRefused(status)).toBe(false);
  });
});
