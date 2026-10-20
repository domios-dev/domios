import assert from "node:assert/strict";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";

class Recorder {
  static isTypeSupported() { return true; }
  state = "inactive";
  mimeType = "audio/webm";
  onstart: (() => void) | null = null;
  onstop: (() => void) | null = null;
  ondataavailable: ((event: { data: Blob }) => void) | null = null;
  constructor(readonly stream: MediaStream) {}
  start() { this.state = "recording"; this.onstart?.(); }
  stop() {
    this.state = "inactive";
    this.ondataavailable?.({ data: new Blob([new Uint8Array(8192)]) });
    this.onstop?.();
  }
}

async function main() {
  const browser = new Window({ url: "https://host.tail.ts.net/" });
  const originals = new Map<string, PropertyDescriptor | undefined>();
  for (const [name, value] of Object.entries({ window: browser, document: browser.document,
    navigator: browser.navigator, MediaRecorder: Recorder, IS_REACT_ACT_ENVIRONMENT: true })) {
    originals.set(name, Object.getOwnPropertyDescriptor(globalThis, name));
    Object.defineProperty(globalThis, name, { configurable: true, value });
  }
  let requests = 0;
  const tracks: Array<{ kind: string; readyState: string; enabled: boolean; stops: number; stop(): void }> = [];
  Object.defineProperty(browser.navigator, "mediaDevices", { configurable: true, value: {
    getUserMedia: async () => {
      requests++;
      const track = { kind: "audio", readyState: "live", enabled: true, stops: 0,
        stop() { this.readyState = "ended"; this.stops++; } };
      tracks.push(track);
      return { getTracks: () => [track], getAudioTracks: () => [track] };
    },
  } });
  const { useVoiceRecorder } = await import("./useVoiceRecorder");
  const errors: string[] = [];
  const persisted: Blob[] = [];
  function Composer() {
    const recorder = useVoiceRecorder((error) => errors.push(error), async (blob) => {
      persisted.push(blob);
      return "voice.webm";
    });
    return <button onClick={() => { if (recorder.recording) void recorder.stop(); else void recorder.start(); }}>
      {recorder.recording ? "Stop recording" : "Record voice"}
    </button>;
  }
  const host = browser.document.createElement("div");
  browser.document.body.append(host);
  const root = createRoot(host as unknown as Element);
  const tap = () => act(async () => host.querySelector("button")!.click());
  try {
    await act(async () => root.render(<Composer />));
    for (let take = 0; take < 2; take++) {
      await tap();
      assert.equal(host.querySelector("button")!.textContent, "Stop recording");
      assert.equal(tracks[0].enabled, true);
      await tap();
      assert.equal(host.querySelector("button")!.textContent, "Record voice");
    }
    assert.equal(requests, 1);
    assert.equal(tracks[0].enabled, false);
    assert.equal(tracks[0].stops, 0);
    assert.equal(persisted.length, 2);
    assert(persisted.every((blob) => blob.size === 8192));
    await act(async () => root.render(null));
    await act(async () => root.render(<Composer />));
    await tap();
    assert.equal(requests, 1, "composer remount reuses the page stream");
    await act(async () => browser.dispatchEvent(new browser.Event("pagehide")));
    assert.equal(tracks[0].stops, 1);
    assert.equal(host.querySelector("button")!.textContent, "Record voice");
    await tap();
    assert.equal(requests, 2, "returning to a hidden page obtains a fresh stream");
    await tap();
    await act(async () => root.render(null));
    browser.dispatchEvent(new browser.Event("pagehide"));
    assert.equal(tracks[1].stops, 1, "page cleanup survives composer unmount");
    assert.deepEqual(errors, []);
    console.log("useVoiceRecorder: 4 browser wiring fixtures passed");
  } finally {
    await act(async () => root.unmount());
    browser.happyDOM.abort();
    for (const [name, descriptor] of originals) {
      if (descriptor) Object.defineProperty(globalThis, name, descriptor);
      else Reflect.deleteProperty(globalThis, name);
    }
  }
}

main().catch((error) => { console.error(error); process.exitCode = 1; });
