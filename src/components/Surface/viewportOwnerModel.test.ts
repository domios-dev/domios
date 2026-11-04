import assert from "node:assert/strict";
import { browserViewerTarget, ownsViewport } from "./viewportOwnerModel";
import { createViewportResizer } from "./viewportResizer";
import type { StreamViewport } from "./streamModel";

assert.equal(ownsViewport("outer", "outer"), true);
assert.equal(ownsViewport("inner", "outer"), false);
assert.equal(ownsViewport("outer", undefined), false);

async function run() {
  const requests: StreamViewport[] = [];
  const resizer = createViewportResizer(async (viewport) => { requests.push(viewport); }, (error) => { throw error; });
  const viewport = { w: 1034, h: 1150, dpr: 1 };
  resizer.push(viewport);
  await Promise.resolve();
  resizer.push({ ...viewport });
  assert.deepEqual(requests, [viewport], "an acknowledged identical viewport is never sent again");
  resizer.dispose();
}
void run();

assert.equal(browserViewerTarget({}), undefined);
assert.equal(browserViewerTarget({ [Symbol.for("domios.browser.target")]: "page-1" }), "page-1");
