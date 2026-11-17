import assert from "node:assert/strict";
import { test } from "node:test";
import { imageCanvasKey } from "./imageCanvasCamera";

test("gallery keys stay bounded and separate galleries", () => {
  const source = "a".repeat(100_000);
  assert(imageCanvasKey([{ src: source }]).length < 40);
  assert.notEqual(imageCanvasKey([{ src: "a" }, { src: "b" }]), imageCanvasKey([{ src: "b" }, { src: "a" }]));
  assert.equal(imageCanvasKey([{ path: "/image.png", src: "" }]), imageCanvasKey([{ path: "/image.png", src: source }]));
});
