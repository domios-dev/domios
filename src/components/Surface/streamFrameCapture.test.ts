import assert from "node:assert/strict";
import { createStreamFrameCapture } from "./streamFrameCapture";
import type { DecodedFrame } from "./frameFeed";
import type { StreamFrame } from "./surfaceStreamTransport";
import { testSuite } from "./surfaceTestDom.testdata";

const suite = testSuite("streamFrameCapture");
const jpeg = new Uint8Array([0xff, 0xd8, 0xff, 0xc0, 0, 11, 8, 0, 6, 0, 8, 1, 1, 0x11, 0, 0xff, 0xd9]);
const decoded: DecodedFrame = { width: 16, height: 12, image: {} as CanvasImageSource, close: () => assert.fail("capture must not close the live frame") };
const css = { width: 400, height: 300 };
const viewport = { w: 400, h: 300, dpr: 2 };
const next = (): StreamFrame => ({ data: jpeg.slice().buffer, metadata: { deviceWidth: 400, deviceHeight: 300, timestamp: 123 } });

function fixture() {
  const stage = document.createElement("div");
  const canvas = document.createElement("canvas");
  canvas.width = 16;
  canvas.height = 12;
  canvas.getBoundingClientRect = () => new DOMRect(10, 20, 400, 300);
  stage.append(canvas);
  document.body.append(stage);
  let snapshot: BlobCallback | null = null;
  let snapshots = 0;
  canvas.toBlob = (callback) => { snapshots += 1; snapshot = callback; };
  const originalCreate = URL.createObjectURL;
  const originalRevoke = URL.revokeObjectURL;
  const blobs = new Map<string, Blob>();
  const revoked: string[] = [];
  URL.createObjectURL = (blob) => {
    assert.ok(blob instanceof Blob);
    const url = `blob:local-${blobs.size}`;
    blobs.set(url, blob);
    return url;
  };
  URL.revokeObjectURL = (url) => { revoked.push(url); };
  const capture = createStreamFrameCapture(canvas, stage, "createImageBitmap/jpeg");
  const buttons = stage.querySelectorAll("button");
  return {
    stage, capture, blobs, revoked,
    arm: () => (buttons[0] as HTMLButtonElement).click(),
    discard: () => (buttons[1] as HTMLButtonElement).click(),
    finish: (blob: Blob | null = new Blob(["png"], { type: "image/png" })) => { assert.ok(snapshot); snapshot(blob); },
    snapshots: () => snapshots,
    cleanup() { capture.dispose(); stage.remove(); URL.createObjectURL = originalCreate; URL.revokeObjectURL = originalRevoke; },
  };
}

suite.test("idle capture does not read or copy a frame", () => {
  const f = fixture();
  try {
    const unread = { get data(): string { return assert.fail("no frame bytes without opt-in"); }, metadata: next().metadata };
    f.capture.capture(unread, decoded, css, viewport, 2);
    assert.equal(f.snapshots(), 0);
    assert.equal(f.blobs.size, 0);
  } finally { f.cleanup(); }
});

suite.test("one arm retains the exact accepted JPEG and a bounded snapshot of dimensions and metadata", async () => {
  const f = fixture();
  try {
    const frame = next();
    Object.assign(frame.metadata, { url: "private", body: "private", offsetTop: Infinity });
    f.arm();
    f.capture.capture(frame, decoded, css, viewport, 2);
    new Uint8Array(frame.data as ArrayBuffer).fill(0);
    frame.metadata.timestamp = 999;
    f.capture.capture(next(), decoded, css, viewport, 2);
    assert.equal(f.snapshots(), 1);
    f.finish();
    const [raw, png, json] = [...f.blobs.values()];
    assert.deepEqual(new Uint8Array(await raw.arrayBuffer()), jpeg);
    assert.equal(await png.text(), "png");
    const metrics = JSON.parse(await json.text());
    assert.deepEqual(metrics.encoded, { width: 8, height: 6 });
    assert.deepEqual(metrics.decoded, { width: 16, height: 12 });
    assert.deepEqual(metrics.backing, { width: 16, height: 12 });
    assert.deepEqual(metrics.cssRect, { x: 10, y: 20, width: 400, height: 300 });
    assert.deepEqual(metrics.requestedViewport, viewport);
    assert.equal(metrics.dpr, window.devicePixelRatio);
    assert.equal(metrics.layoutDpr, 2);
    assert.equal(metrics.acceptedDraw, 1);
    assert.equal(metrics.decodeBackend, "createImageBitmap/jpeg");
    assert.equal(metrics.metadata.timestamp, 123);
    assert.equal(metrics.metadata.offsetTop, null);
    assert.deepEqual(Object.keys(metrics.metadata).sort(), ["deviceWidth", "deviceHeight", "offsetTop", "pageScaleFactor", "scrollOffsetX", "scrollOffsetY", "timestamp"].sort());
    assert.ok(!JSON.stringify(metrics).includes("private"));
    f.arm();
    f.capture.capture(next(), decoded, css, viewport, 2);
    assert.equal(f.snapshots(), 1, "captured state cannot accumulate another image");
    assert.equal(f.stage.querySelectorAll("a[download]").length, 3);
    assert.equal(f.blobs.size, 3, "nothing downloads or uploads automatically");
    f.discard();
    assert.equal(f.stage.querySelectorAll("a").length, 0);
    assert.equal(f.revoked.length, 3);
  } finally { f.cleanup(); }
});

suite.test("discard cancels a pending snapshot and a fresh arm owns its own result", () => {
  const f = fixture();
  try {
    f.arm();
    f.capture.capture(next(), decoded, css, viewport, 2);
    f.discard();
    f.finish();
    assert.equal(f.blobs.size, 0);
    f.arm();
    f.capture.capture(next(), decoded, css, viewport, 2);
    f.finish();
    assert.equal(f.snapshots(), 2);
    assert.equal(f.blobs.size, 3);
  } finally { f.cleanup(); }
});

suite.test("unmount releases all artifact URLs and refuses late snapshots", () => {
  const f = fixture();
  try {
    f.arm();
    f.capture.capture(next(), decoded, css, viewport, 2);
    f.finish();
    f.capture.dispose();
    assert.equal(f.revoked.length, 3);
    assert.equal(f.stage.querySelector("button"), null);
    f.finish();
    assert.equal(f.blobs.size, 3);
    f.capture.capture(next(), decoded, css, viewport, 2);
    assert.equal(f.snapshots(), 1);
  } finally { f.cleanup(); }
});

suite.test("unmount before PNG completion does not publish or retain download URLs", () => {
  const f = fixture();
  try {
    f.arm();
    f.capture.capture(next(), decoded, css, viewport, 2);
    f.capture.dispose();
    f.finish();
    assert.equal(f.blobs.size, 0);
    assert.equal(f.stage.querySelector("a"), null);
  } finally { f.cleanup(); }
});

suite.test("snapshot failure exports no partial artifacts and malformed JPEG sizes remain unknown", async () => {
  const f = fixture();
  try {
    f.arm();
    f.capture.capture(next(), decoded, css, viewport, 2);
    f.finish(null);
    assert.equal(f.blobs.size, 0);
    f.discard();
    f.arm();
    f.capture.capture({ data: "AA==", metadata: next().metadata }, decoded, css, null, 2);
    f.finish();
    const json = [...f.blobs.values()][2];
    const metrics = JSON.parse(await json.text());
    assert.equal(metrics.encoded, null);
    assert.equal(metrics.requestedViewport, null);
  } finally { f.cleanup(); }
});

void suite.run();
