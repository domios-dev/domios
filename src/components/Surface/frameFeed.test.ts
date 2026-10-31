// Run: npx tsx src/components/Surface/frameFeed.test.ts

import assert from "node:assert/strict";
import { createFrameFeed, decodeJpegFrame, frameBytes } from "./frameFeed";
import type { StreamFrame } from "./surfaceStreamTransport";

const frame = (data: StreamFrame["data"]): StreamFrame => ({ data, metadata: { deviceWidth: 1, deviceHeight: 1 } });

const feed = createFrameFeed();
const seen: StreamFrame[] = [];
const unsubscribe = feed.subscribe((next) => seen.push(next));
const first = frame("AA==");
feed.publish(first);
unsubscribe();
feed.publish(frame("AQ=="));
assert.deepEqual(seen, [first], "listeners receive frames until they unsubscribe");

assert.deepEqual([...frameBytes(btoa("\x01\x02"))], [1, 2], "base64 frames decode to bytes");
assert.deepEqual([...frameBytes(new Uint8Array([3, 4]).buffer)], [3, 4], "binary frames pass through");

const resumed: StreamFrame[] = [];
const stopResumed = feed.subscribe((next) => resumed.push(next));
assert.deepEqual(resumed, [frame("AQ==")], "returning viewers get the current frame without waiting for a repaint");
stopResumed();

let allowed = true;
const guarded = createFrameFeed(() => allowed);
const accepted: StreamFrame[] = [];
guarded.subscribe((next) => accepted.push(next));
guarded.publish(first);
allowed = false;
guarded.publish(frame("AQ=="));
const replayed: StreamFrame[] = [];
guarded.subscribe((next) => replayed.push(next));
assert.deepEqual(accepted, [first], "rejected frames never replace the last good frame");
assert.deepEqual(replayed, [], "a cached frame is revalidated when a viewer returns");

function decoderFixture(supported = true) {
  const previousImage = Object.getOwnPropertyDescriptor(globalThis, "Image");
  const originalCreate = URL.createObjectURL;
  const originalRevoke = URL.revokeObjectURL;
  const images: ImageStub[] = [];
  const blobs: Blob[] = [];
  const revoked: string[] = [];
  class ImageStub {
    src = "";
    width = 1;
    height = 1;
    naturalWidth = 1600;
    naturalHeight = 900;
    complete!: () => void;
    fail!: (error: Error) => void;
    private pending = new Promise<void>((resolve, reject) => { this.complete = resolve; this.fail = reject; });
    constructor() { images.push(this); }
    decode() {
      assert.ok(this.src.startsWith("blob:"), "a local image source is assigned before decode");
      return this.pending;
    }
  }
  Object.defineProperty(globalThis, "Image", { value: supported ? ImageStub : class {}, configurable: true });
  URL.createObjectURL = (blob) => {
    assert.ok(blob instanceof Blob);
    blobs.push(blob);
    return `blob:jpeg-${blobs.length}`;
  };
  URL.revokeObjectURL = (url) => { revoked.push(url); };
  return {
    images, blobs, revoked,
    cleanup() {
      if (previousImage) Object.defineProperty(globalThis, "Image", previousImage);
      else Reflect.deleteProperty(globalThis, "Image");
      URL.createObjectURL = originalCreate;
      URL.revokeObjectURL = originalRevoke;
    },
  };
}

async function runDecoderTests() {
  const success = decoderFixture();
  try {
    const pending = decodeJpegFrame(frame(new Uint8Array([0xff, 0xd8, 0xff, 0xd9]).buffer));
    assert.equal(success.images.length, 1);
    assert.equal(success.blobs.length, 1, "one decode owns exactly one JPEG Blob URL");
    assert.equal(success.blobs[0].type, "image/jpeg");
    assert.deepEqual([...new Uint8Array(await success.blobs[0].arrayBuffer())], [0xff, 0xd8, 0xff, 0xd9]);
    assert.deepEqual(success.revoked, [], "the in-flight source stays available");
    success.images[0].complete();
    const decoded = await pending;
    assert.equal(decoded.width, 1600, "frame size comes from natural pixels rather than element width");
    assert.equal(decoded.height, 900);
    assert.equal(decoded.image, success.images[0]);
    assert.deepEqual(success.revoked, [], "the retained good frame owns its URL until close");
    decoded.close();
    decoded.close();
    assert.deepEqual(success.revoked, ["blob:jpeg-1"], "close revokes once");
    assert.equal(success.images[0].src, "blob:jpeg-1", "close does not invalidate the drawable image source");
  } finally { success.cleanup(); }

  const failure = decoderFixture();
  try {
    const error = new Error("opaque JPEG decode failure");
    const pending = decodeJpegFrame(frame("/9j/2Q=="));
    const rejected = assert.rejects(pending, (received) => received === error);
    failure.images[0].fail(error);
    await rejected;
    assert.deepEqual(failure.revoked, ["blob:jpeg-1"], "failed decode releases its own source URL");
    const next = decodeJpegFrame(frame("/9j/2Q=="));
    failure.images[1].complete();
    const recovered = await next;
    assert.deepEqual(failure.revoked, ["blob:jpeg-1"]);
    recovered.close();
    assert.deepEqual(failure.revoked, ["blob:jpeg-1", "blob:jpeg-2"], "a failed decode does not prevent a later decode");
  } finally { failure.cleanup(); }

  const unsupported = decoderFixture(false);
  try {
    await assert.rejects(decodeJpegFrame(frame("AA==")), /JPEG image decoding is unavailable/);
    assert.equal(unsupported.blobs.length, 0, "unsupported decode rejects before allocating an image URL");
    assert.deepEqual(unsupported.revoked, []);
  } finally { unsupported.cleanup(); }

  console.log("✓ frameFeed feed assertions and 3 JPEG decoder lifecycle cases green");
}

void runDecoderTests().catch((error: unknown) => { console.error(error); process.exitCode = 1; });
