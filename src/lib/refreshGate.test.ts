import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createRefreshGate } from "./refreshGate";

let frames = new Map<number, FrameRequestCallback>();
let nextFrame = 0;

function runFrames() {
  const callbacks = [...frames.values()];
  frames.clear();
  for (const callback of callbacks) callback(0);
}

beforeEach(() => {
  frames = new Map();
  nextFrame = 0;
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
    const id = ++nextFrame;
    frames.set(id, callback);
    return id;
  });
  vi.stubGlobal("cancelAnimationFrame", (id: number) => frames.delete(id));
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("refresh gate", () => {
  it("runs once for a burst of requests inside one frame", () => {
    const run = vi.fn();
    const gate = createRefreshGate(run);
    for (let i = 0; i < 5; i++) gate.request();
    expect(frames.size).toBe(1);
    expect(run).not.toHaveBeenCalled();
    runFrames();
    expect(run).toHaveBeenCalledTimes(1);
    runFrames();
    expect(run).toHaveBeenCalledTimes(1);
  });

  it("holds requests during an in-flight read to one trailing run", () => {
    const run = vi.fn();
    const gate = createRefreshGate(run);
    gate.request();
    runFrames();
    for (let i = 0; i < 4; i++) gate.request();
    expect(frames.size).toBe(0);
    gate.settled();
    expect(frames.size).toBe(1);
    runFrames();
    expect(run).toHaveBeenCalledTimes(2);
    gate.settled();
    expect(frames.size).toBe(0);
  });

  it("drops a scheduled or queued run on cancel", () => {
    const run = vi.fn();
    const gate = createRefreshGate(run);
    gate.request();
    gate.cancel();
    runFrames();
    expect(run).not.toHaveBeenCalled();

    gate.request();
    runFrames();
    gate.request();
    gate.cancel();
    gate.settled();
    expect(frames.size).toBe(0);
    expect(run).toHaveBeenCalledTimes(1);
  });
});
