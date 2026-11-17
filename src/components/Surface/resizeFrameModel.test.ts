import assert from "node:assert/strict";
import { acceptViewportFrame } from "./resizeFrameModel";

const a = { w: 640, h: 480, dpr: 1 };
assert.equal(acceptViewportFrame({ deviceWidth: 640, deviceHeight: 480 }, a), true);
assert.equal(acceptViewportFrame({ deviceWidth: 800, deviceHeight: 600 }, a), false);
assert.equal(acceptViewportFrame({ deviceWidth: 640, deviceHeight: 600 }, a), false);
assert.equal(acceptViewportFrame({ deviceWidth: 320, deviceHeight: 240 }, { w: 200, h: 100, dpr: 3 }), true, "backend minimum dimensions apply");
assert.equal(acceptViewportFrame({ deviceWidth: 640, deviceHeight: 480 }, null), false);
