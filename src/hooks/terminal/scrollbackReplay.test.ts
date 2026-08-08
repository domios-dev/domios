import { describe, expect, finish, it, vi } from "../../test/standaloneTest";
import {
  replayScrollbackBeforeLiveAttach,
  shouldReplayScrollbackSidecar,
} from "./scrollbackReplay";

describe("scrollback sidecar replay gating", () => {
  it("owns only restored local tabs without a live pty", () => {
    expect(shouldReplayScrollbackSidecar(null, false, false)).toBe(true);
    expect(shouldReplayScrollbackSidecar(undefined, false, false)).toBe(true);
  });

  it("leaves live-buffer, web, and remote replay to their owners", () => {
    expect(shouldReplayScrollbackSidecar("pty", false, false)).toBe(false);
    expect(shouldReplayScrollbackSidecar(null, true, false)).toBe(false);
    expect(shouldReplayScrollbackSidecar(null, false, true)).toBe(false);
  });
});

describe("scrollback replay ordering", () => {
  it("completes replay before live output attaches", async () => {
    const events: string[] = [];
    let completeReplay: (() => void) | null = null;
    const settled = replayScrollbackBeforeLiveAttach({
      loadTail: async () => new Uint8Array([104, 105]),
      isDisposed: () => false,
      replay: (tail, onComplete) => {
        events.push(`replay:${tail.length}`);
        completeReplay = onComplete;
      },
    }).then((outcome) => {
      events.push("live-attach");
      return outcome;
    });

    await Promise.resolve();
    await Promise.resolve();
    expect(events).toEqual(["replay:2"]);
    expect(completeReplay).not.toBeNull();

    completeReplay!();
    expect(await settled).toBe("replayed");
    expect(events).toEqual(["replay:2", "live-attach"]);
  });

  it("attaches live immediately when there is no sidecar tail", async () => {
    const replay = vi.fn();
    const outcome = await replayScrollbackBeforeLiveAttach({
      loadTail: async () => null,
      isDisposed: () => false,
      replay: replay as never,
    });
    expect(outcome).toBe("empty");
    expect(replay).not.toHaveBeenCalled();
  });

  it("treats a failing sidecar load as blank tab, never an error", async () => {
    const replay = vi.fn();
    const outcome = await replayScrollbackBeforeLiveAttach({
      loadTail: async () => {
        throw new Error("corrupt sidecar");
      },
      isDisposed: () => false,
      replay: replay as never,
    });
    expect(outcome).toBe("empty");
    expect(replay).not.toHaveBeenCalled();
  });

  it("cancels without replaying when the tab unmounts mid-load", async () => {
    const replay = vi.fn();
    const outcome = await replayScrollbackBeforeLiveAttach({
      loadTail: async () => new Uint8Array([1]),
      isDisposed: () => true,
      replay: replay as never,
    });
    expect(outcome).toBe("cancelled");
    expect(replay).not.toHaveBeenCalled();
  });

  it("resolves cancelled when the replay itself is torn down", async () => {
    const outcome = await replayScrollbackBeforeLiveAttach({
      loadTail: async () => new Uint8Array([1, 2, 3]),
      isDisposed: () => false,
      replay: (_tail, _onComplete, onCancel) => onCancel(),
    });
    expect(outcome).toBe("cancelled");
  });
});

finish();
