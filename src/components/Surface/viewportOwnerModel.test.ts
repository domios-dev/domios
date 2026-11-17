import assert from "node:assert/strict";
import { browserViewerTarget } from "./viewportOwnerModel";

assert.equal(browserViewerTarget({}), undefined);
assert.equal(browserViewerTarget({ [Symbol.for("domios.browser.target")]: "page-1" }), "page-1");
