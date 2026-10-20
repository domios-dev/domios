import { test } from "node:test";
import assert from "node:assert/strict";
import { frontendDiagnosticStamp } from "./frontendDiagnosticStamp";

const input = { userAgent: "Version/17.0 Safari/605.1", width: 390, height: 844, visualWidth: 390, visualHeight: 700,
  pixelRatio: 3, touch: true, standalone: true, scrollEnd: false, overflowAnchor: false };
test("stamp carries loaded identity and device metadata without URL or UA content", () => {
  const stamp = frontendDiagnosticStamp("abcdef123", "https://private.test/assets/index-AbCd.js?token=secret", input);
  assert.equal(stamp.buildSha, "abcdef123");
  assert.deepEqual(stamp.device, { entryAsset: "index-AbCd.js", userAgentFamily: "safari", width: 390, height: 844,
    visualWidth: 390, visualHeight: 700, pixelRatio: 3, touch: true, displayMode: "standalone", scrollEnd: false, overflowAnchor: false });
});
test("stamp bounds malformed identity and numeric input", () => {
  const stamp = frontendDiagnosticStamp("private content", "https://host/private name.js", { ...input, width: Infinity, height: -1, visualWidth: 1e8, pixelRatio: 99 });
  assert.equal(stamp.buildSha, undefined);
  assert.equal(stamp.device.entryAsset, undefined);
  assert.equal(stamp.device.width, 0);
  assert.equal(stamp.device.height, 0);
  assert.equal(stamp.device.visualWidth, 32768);
  assert.equal(stamp.device.pixelRatio, 16);
});
test("app version is bounded metadata", () => {
  assert.equal(frontendDiagnosticStamp("abcdef123", "index.js", { ...input, appVersion: "1.14.9" }).device.appVersion, "1.14.9");
  assert.equal(frontendDiagnosticStamp("abcdef123", "index.js", { ...input, appVersion: "private content" }).device.appVersion, undefined);
});
