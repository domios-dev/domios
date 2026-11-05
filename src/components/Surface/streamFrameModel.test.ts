import assert from "node:assert/strict";
import { viewportFrame } from "./streamFrameModel";
import { canvasToPage, letterboxRect, stageRectToPage } from "./streamModel";

const metadata = { deviceWidth: 800, deviceHeight: 600, offsetTop: 100, pageScaleFactor: 2, scrollOffsetY: 45 };
let cases = 0;

for (const ratio of [0.5, 1, 1.25, 1.5, 2]) {
  const frame = { width: 800 * ratio, height: 600 * ratio };
  const cropped = viewportFrame(frame, metadata);
  assert.deepEqual(cropped.source, { x: 0, y: 100 * ratio, width: 800 * ratio, height: 500 * ratio });
  assert.deepEqual(cropped.metadata, { ...metadata, offsetTop: 0, deviceHeight: 500 });
  const stage = { width: 800, height: 800 };
  const draw = letterboxRect(cropped.source, stage);
  assert.deepEqual(draw, { x: 0, y: 150, width: 800, height: 500 }, "only the cropped viewport participates in letterboxing");
  const canvas = { left: 0, top: 0, ...stage };
  assert.deepEqual(canvasToPage(0, 150, canvas, draw, cropped.metadata), { x: 0, y: 0 });
  assert.deepEqual(canvasToPage(800, 650, canvas, draw, cropped.metadata), { x: 400, y: 250 });
  assert.equal(canvasToPage(400, 149, canvas, draw, cropped.metadata), null);
  assert.deepEqual(stageRectToPage({ x: 0, y: 150 }, { x: 800, y: 650 }, draw, cropped.metadata), { x: 0, y: 45, width: 400, height: 250 });
  cases += 1;
}

for (const offsetTop of [undefined, 0, -1, 600, 700, NaN, Infinity]) {
  const input = { ...metadata, offsetTop };
  assert.deepEqual(viewportFrame({ width: 800, height: 600 }, input), {
    source: { x: 0, y: 0, width: 800, height: 600 }, metadata: input,
  }, "absent or invalid insets retain the full frame and its coordinate system");
  cases += 1;
}

for (const deviceHeight of [0, NaN, Infinity]) {
  const input = { ...metadata, deviceHeight };
  assert.equal(viewportFrame({ width: 800, height: 600 }, input).metadata, input);
  cases += 1;
}

assert.deepEqual(viewportFrame({ width: 1000, height: 751 }, { ...metadata, offsetTop: 37.5 }).source,
  { x: 0, y: 47, width: 1000, height: 704 }, "fractional source boundaries round to bitmap pixels");
assert.equal(metadata.offsetTop, 100, "cropping never mutates transport metadata used for viewport matching");
console.log(`streamFrameModel: ${cases + 2} cases passed`);
