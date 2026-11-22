import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { LiveCaptureAdmission, LiveCapturePauseReason, LiveCaptureUpdate } from "../../../lib/transport/liveTranscriptCapture";
import { claimCaptureOwnership, releaseCaptureOwnership, resetCaptureOwnershipForTest } from "./localRecordingCaptureModel";
import { useLiveTranscriptCapture } from "./useLiveTranscriptCapture";

const transport = vi.hoisted(() => ({
  assertCurrent: vi.fn(),
  start: vi.fn<(resume: boolean, signal: AbortSignal) => Promise<LiveCaptureAdmission>>(),
  frame: vi.fn<(lease: string, sequence: number, pcm: ArrayBuffer) => Promise<LiveCaptureUpdate>>(),
  pause: vi.fn<(lease: string, sequence: number, reason: LiveCapturePauseReason) => Promise<LiveCaptureUpdate>>(),
  cancel: vi.fn(async () => {}),
}));

vi.mock("../../../lib/transport/liveTranscriptCapture", () => ({
  createLiveTranscriptCaptureTransport: () => transport,
}));

function deferred<T>() {
  let fulfill!: (value: T) => void;
  const promise = new Promise<T>(resolve => { fulfill = resolve; });
  return { promise, fulfill };
}

function stream() {
  const track = Object.assign(new EventTarget(), { stop: vi.fn() });
  const media = { getTracks: () => [track], getAudioTracks: () => [track] } as unknown as MediaStream;
  return { media, track };
}

class Worklet {
  static current: Worklet;
  tail?: ArrayBuffer;
  port = {
    onmessage: (_: MessageEvent<unknown>) => {},
    postMessage: (message: string) => {
      if (message !== "seal") return;
      if (this.tail) this.emit(this.tail);
      this.emit("sealed");
    },
    close: vi.fn(),
  };
  connect = vi.fn();
  disconnect = vi.fn();

  constructor() { Worklet.current = this; }

  emit(data: unknown) { this.port.onmessage(new MessageEvent("message", { data })); }
}

class Audio {
  readonly sampleRate = 16_000;
  readonly destination = {};
  readonly audioWorklet = { addModule: async () => {} };
  createMediaStreamSource() {
    return { connect: (node: Worklet) => node, disconnect: vi.fn() };
  }
  async resume() {}
  async close() {}
}

let hasFocus: boolean;
let visibility: DocumentVisibilityState;
const getUserMedia = vi.fn<() => Promise<MediaStream>>();
const previousAct = (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT;

beforeEach(() => {
  resetCaptureOwnershipForTest();
  vi.resetAllMocks();
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal("AudioWorkletNode", Worklet);
  vi.stubGlobal("AudioContext", Audio);
  vi.stubGlobal("URL", class extends URL {
    static createObjectURL = () => "blob:microphone";
    static revokeObjectURL = () => {};
  });
  hasFocus = true;
  visibility = "visible";
  vi.spyOn(document, "hasFocus").mockImplementation(() => hasFocus);
  vi.spyOn(document, "visibilityState", "get").mockImplementation(() => visibility);
  vi.stubGlobal("navigator", Object.create(navigator, { mediaDevices: { value: { getUserMedia } } }));
  transport.start.mockResolvedValue({ state: "ready", lease: "first" });
  transport.frame.mockImplementation(async (lease, sequence) => ({ lease, sequence, provisional: "interim", paused: false }));
  transport.pause.mockImplementation(async (lease, sequence) => ({ lease, sequence, provisional: "", paused: true }));
});

afterEach(() => {
  resetCaptureOwnershipForTest();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = previousAct;
});

async function mount(autoResume = false) {
  const root = createRoot(document.createElement("div"));
  let capture!: ReturnType<typeof useLiveTranscriptCapture>;
  function Harness() {
    capture = useLiveTranscriptCapture({ tabId: "tab", identity: "generation-1", enabled: true, autoResume });
    return null;
  }
  await act(async () => { root.render(<Harness />); });
  return { render: () => capture, unmount: () => act(async () => { root.unmount(); }) };
}

describe("live transcript microphone ownership", () => {
  it("releases tracks on blur before draining queued audio, then waits for a manual resume", async () => {
    const { media, track } = stream();
    const inFlight = deferred<LiveCaptureUpdate>();
    const finishing = deferred<LiveCaptureUpdate>();
    getUserMedia.mockResolvedValue(media);
    transport.frame.mockReturnValueOnce(inFlight.promise);
    transport.pause.mockReturnValueOnce(finishing.promise);
    const harness = await mount();
    try {
      await act(async () => { await harness.render().start(); });
      expect(harness.render().phase).toBe("recording");
      const frame = new ArrayBuffer(16_000);
      const tail = new ArrayBuffer(320);
      Worklet.current.tail = tail;
      await act(async () => { Worklet.current.emit(frame); });
      await act(async () => {
        hasFocus = false;
        window.dispatchEvent(new Event("blur"));
        expect(track.stop).toHaveBeenCalledOnce();
      });
      expect(harness.render().phase).toBe("pausing");
      expect(transport.pause).not.toHaveBeenCalled();
      await act(async () => {
        inFlight.fulfill({ lease: "first", sequence: 1, provisional: "stale preview", paused: false });
      });
      expect(transport.frame.mock.calls.map(([lease, sequence, bytes]) => [lease, sequence, bytes])).toEqual([
        ["first", 1, frame], ["first", 2, tail],
      ]);
      expect(transport.pause).toHaveBeenCalledExactlyOnceWith("first", 3, "focusLost", expect.any(AbortSignal));
      expect(harness.render().provisional).toBe("");
      await act(async () => {
        hasFocus = true;
        window.dispatchEvent(new Event("focus"));
        finishing.fulfill({ lease: "first", sequence: 3, provisional: "", paused: true });
      });
      expect(harness.render().phase).toBe("paused");
      expect(getUserMedia).toHaveBeenCalledOnce();
      const resumed = stream();
      getUserMedia.mockResolvedValueOnce(resumed.media);
      transport.start.mockResolvedValueOnce({ state: "ready", lease: "second" });
      await act(async () => { await harness.render().start(); });
      expect(getUserMedia).toHaveBeenCalledTimes(2);
      expect(transport.start).toHaveBeenLastCalledWith(true, expect.any(AbortSignal));
      expect(harness.render().phase).toBe("recording");
    } finally {
      await harness.unmount();
    }
  });

  it("pauses a hidden lease while permission is pending and stops every late track", async () => {
    const permission = deferred<MediaStream>();
    const entered = deferred<void>();
    const late = stream();
    getUserMedia.mockImplementation(() => {
      entered.fulfill();
      return permission.promise;
    });
    const harness = await mount();
    try {
      let starting!: Promise<void>;
      await act(async () => {
        starting = harness.render().start();
        await entered.promise;
      });
      await act(async () => {
        visibility = "hidden";
        document.dispatchEvent(new Event("visibilitychange"));
      });
      expect(transport.pause).toHaveBeenCalledExactlyOnceWith("first", 1, "hidden", expect.any(AbortSignal));
      expect(harness.render().phase).toBe("paused");
      expect(claimCaptureOwnership("manual")).toBe(false);
      await act(async () => {
        visibility = "visible";
        document.dispatchEvent(new Event("visibilitychange"));
        await harness.render().start();
        permission.fulfill(late.media);
        await starting;
      });
      expect(late.track.stop).toHaveBeenCalledOnce();
      expect(getUserMedia).toHaveBeenCalledOnce();
      expect(transport.frame).not.toHaveBeenCalled();
      expect(claimCaptureOwnership("manual")).toBe(true);
      releaseCaptureOwnership("manual");
    } finally {
      await harness.unmount();
    }
  });

  it("observes an opted-in focus resume only after the original pause acknowledgment", async () => {
    const first = stream();
    const second = stream();
    getUserMedia.mockResolvedValueOnce(first.media).mockResolvedValueOnce(second.media);
    transport.start.mockResolvedValueOnce({ state: "ready", lease: "first" }).mockResolvedValueOnce({ state: "ready", lease: "second" });
    const finishing = deferred<LiveCaptureUpdate>();
    transport.pause.mockReturnValueOnce(finishing.promise);
    const harness = await mount(true);
    try {
      await act(async () => { await harness.render().start(); });
      await act(async () => {
        hasFocus = false;
        window.dispatchEvent(new Event("blur"));
      });
      await act(async () => {
        hasFocus = true;
        window.dispatchEvent(new Event("focus"));
      });
      expect(getUserMedia).toHaveBeenCalledOnce();
      await act(async () => { finishing.fulfill({ lease: "first", sequence: 1, provisional: "", paused: true }); });
      expect(getUserMedia).toHaveBeenCalledTimes(2);
      expect(transport.start).toHaveBeenLastCalledWith(true, expect.any(AbortSignal));
      expect(harness.render().phase).toBe("recording");
    } finally {
      await harness.unmount();
    }
  });

  it("releases a stale lease immediately on a refused frame and preserves the error", async () => {
    const { media, track } = stream();
    getUserMedia.mockResolvedValue(media);
    transport.frame.mockRejectedValueOnce(new Error("The live capture lease has ended."));
    const harness = await mount();
    try {
      await act(async () => { await harness.render().start(); });
      await act(async () => { Worklet.current.emit(new ArrayBuffer(16_000)); });
      expect(track.stop).toHaveBeenCalledOnce();
      expect(transport.cancel).toHaveBeenCalledExactlyOnceWith("first");
      expect(transport.pause).not.toHaveBeenCalled();
      expect(harness.render().phase).toBe("error");
      expect(harness.render().error).toContain("lease has ended");
    } finally {
      await harness.unmount();
    }
  });
});
