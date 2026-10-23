import { describe, expect, it } from "vitest";
import {
  EMPTY_MUX_CHANNEL_SET, decodeMuxFrame, isMuxStreamPath, muxAddChannel, muxRemoveChannel, muxStreamEnd,
  muxStreamEnded, muxStreamReady,
} from "./streamMuxModel";
import { TransportError } from "./transportClient";

describe("stream mux channel set", () => {
  it("opens one stream for the first channel, requests channels only once it is ready, and closes with the last", () => {
    let step = muxAddChannel(EMPTY_MUX_CHANNEL_SET, "a");
    expect(step.effects).toEqual([{ type: "open-stream" }]);
    step = muxAddChannel(step.state, "b");
    expect(step.effects).toEqual([]);
    step = muxStreamReady(step.state);
    expect(step.effects).toEqual([{ type: "request-channel", id: "a" }, { type: "request-channel", id: "b" }]);
    expect(muxStreamReady(step.state).effects).toEqual([]);
    step = muxAddChannel(step.state, "c");
    expect(step.effects).toEqual([{ type: "request-channel", id: "c" }]);
    step = muxRemoveChannel(step.state, "b");
    expect(step.effects).toEqual([{ type: "release-channel", id: "b" }]);
    step = muxRemoveChannel(step.state, "c", true);
    expect(step.effects).toEqual([]);
    expect(muxRemoveChannel(step.state, "missing")).toEqual({ state: step.state, effects: [] });
    step = muxRemoveChannel(step.state, "a");
    expect(step).toEqual({ state: EMPTY_MUX_CHANNEL_SET, effects: [{ type: "close-stream" }] });
  });

  it("never releases a channel that was not yet requested", () => {
    let step = muxAddChannel(EMPTY_MUX_CHANNEL_SET, "a");
    step = muxAddChannel(step.state, "b");
    step = muxRemoveChannel(step.state, "a");
    expect(step.effects).toEqual([]);
    expect(muxStreamReady(step.state).effects).toEqual([{ type: "request-channel", id: "b" }]);
  });

  it("ends every channel with the shared stream and starts the next stream empty", () => {
    let step = muxAddChannel(EMPTY_MUX_CHANNEL_SET, "a");
    step = muxAddChannel(muxStreamReady(step.state).state, "b");
    const ended = muxStreamEnded(step.state);
    expect(ended.ended).toEqual(["a", "b"]);
    expect(muxAddChannel(ended.state, "c").effects).toEqual([{ type: "open-stream" }]);
  });

  it("multiplexes relay streams and the aggregate output stream only", () => {
    expect(isMuxStreamPath("/api/mesh/relay-stream?peer=p&path=%2Fapi%2Fevents%2Fstream")).toBe(true);
    expect(isMuxStreamPath("/api/output/stream?since=abc")).toBe(true);
    expect(isMuxStreamPath("/api/events/stream")).toBe(false);
    expect(isMuxStreamPath("/api/stream-mux?session=s")).toBe(false);
    expect(isMuxStreamPath("//evil/api/output/stream")).toBe(false);
  });
});

describe("stream mux frame routing", () => {
  it("decodes tagged frames per channel", () => {
    expect(decodeMuxFrame({ channel: "c", data: { seq: 1 } })).toEqual({ channel: "c", kind: "data", event: "message", data: { seq: 1 } });
    expect(decodeMuxFrame({ channel: "c", event: "tick", data: "x" })).toEqual({ channel: "c", kind: "data", event: "tick", data: "x" });
    expect(decodeMuxFrame({ channel: "c", data: null })).toEqual({ channel: "c", kind: "data", event: "message", data: null });
    expect(decodeMuxFrame({ channel: "c", activity: true })).toEqual({ channel: "c", kind: "activity" });
    expect(decodeMuxFrame({ channel: "c", open: true, content_type: "text/event-stream" })).toEqual({ channel: "c", kind: "open", contentType: "text/event-stream" });
    expect(decodeMuxFrame({ channel: "c", end: { reason: "http", status: 504, body: "peer-unreachable" } }))
      .toEqual({ channel: "c", kind: "end", end: { reason: "http", status: 504, body: "peer-unreachable" } });
    for (const invalid of [null, "x", {}, { channel: "" }, { channel: "c" }, { channel: "c", end: { reason: "later" } }, { channel: "c", end: { reason: "http" } }]) {
      expect(decodeMuxFrame(invalid)).toBeNull();
    }
  });

  it("maps a channel end to the stream end its subscriber already understands", () => {
    expect(muxStreamEnd({ reason: "eof" }, "/p")).toEqual({ reason: "eof" });
    const refused = muxStreamEnd({ reason: "http", status: 404, body: "not paired" }, "/p");
    expect(refused).toMatchObject({ reason: "http", status: 404 });
    expect(refused.error).toBeInstanceOf(TransportError);
    expect(muxStreamEnd({ reason: "capacity", bytes: 9 }, "/p")).toEqual({ reason: "capacity", bytes: 9 });
    expect(muxStreamEnd({ reason: "error", message: "reset" }, "/p")).toMatchObject({ reason: "error", error: new Error("reset") });
  });
});
