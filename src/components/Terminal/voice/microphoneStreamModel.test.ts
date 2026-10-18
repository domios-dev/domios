import assert from "node:assert/strict";
import { createMicrophoneStreamModel } from "./microphoneStreamModel";

function fixture() {
  const track = { readyState: "live", enabled: true, stops: 0,
    stop() { this.stops++; this.readyState = "ended"; } };
  const stream = { getAudioTracks: () => [track], getTracks: () => [track] };
  return { track, stream };
}

async function main() {
  const model = createMicrophoneStreamModel<ReturnType<typeof fixture>["stream"]>();
  const first = fixture();
  let requests = 0;
  const request = async () => { requests++; return first.stream; };
  assert.equal(await model.acquire(request), first.stream);
  model.pause();
  assert.equal(first.track.enabled, false);
  assert.equal(await model.acquire(request), first.stream);
  assert.equal(first.track.enabled, true);
  assert.equal(requests, 1);
  assert.equal(first.track.stops, 0);
  first.track.readyState = "ended";
  const second = fixture();
  assert.equal(await model.acquire(async () => second.stream), second.stream);
  model.release();
  model.release();
  assert.equal(second.track.stops, 1);

  let resolve!: (value: typeof first.stream) => void;
  const late = fixture();
  const pending = model.acquire(() => new Promise((done) => { resolve = done; }));
  assert.equal(model.acquire(request), pending);
  model.release();
  resolve(late.stream);
  await assert.rejects(pending);
  assert.equal(late.track.stops, 1);
  await assert.rejects(model.acquire(async () => { throw new Error("denied"); }));
  const recovered = fixture();
  assert.equal(await model.acquire(async () => recovered.stream), recovered.stream);
  model.release();
  console.log("microphoneStreamModel: 5 lifecycle fixtures passed");
}

main().catch((error) => { console.error(error); process.exitCode = 1; });
