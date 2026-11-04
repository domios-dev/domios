import { describe, expect, it } from "vitest";
import { decodeMuxFrame, isMuxStreamPath } from "./streamMuxModel";

describe("mux stream routes", () => {
  it("shares events, startup, output, relay, agent-run and surface streams", () => {
    for (const path of ["/api/events/stream?since=b:4", "/api/startup/events", "/api/output/stream", "/api/mesh/relay-stream?peer=p", "/api/tabs/t/output/stream?from=3", "/api/surfaces/t/stream"]) expect(isMuxStreamPath(path)).toBe(true);
    for (const path of ["/api/stream-mux?session=s", "//evil/api/output/stream", "/api\\output/stream", "/api/tabs/t/chat-send"]) expect(isMuxStreamPath(path)).toBe(false);
  });
});

describe("mux response codec", () => {
  it("carries HTTP status, filtered headers, UTF-8 and binary bytes, and bounded endings", () => {
    expect(decodeMuxFrame({ channel: "c", open: true, status: 503, headers: [["content-type", "application/json"]] }))
      .toEqual({ channel: "c", kind: "open", status: 503, headers: [["content-type", "application/json"]] });
    expect(decodeMuxFrame({ channel: "c", chunk: "雪", enc: "utf8" })).toEqual({ channel: "c", kind: "chunk", chunk: "雪", enc: "utf8" });
    expect(decodeMuxFrame({ channel: "c", chunk: "/w==", enc: "base64" })).toEqual({ channel: "c", kind: "chunk", chunk: "/w==", enc: "base64" });
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
