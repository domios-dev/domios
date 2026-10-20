import assert from "node:assert/strict";
import { keyboardViewportState } from "./keyboardViewport";

assert.deepEqual(keyboardViewportState({ rootHeight: 800, layoutHeight: 800, visualHeight: 800, offsetTop: 0 }), { inset: 0, keyboardOpen: false, visualHeight: 800, offsetTop: 0 });
assert.deepEqual(keyboardViewportState({ rootHeight: 800, layoutHeight: 800, visualHeight: 450, offsetTop: 30 }), { inset: 320, keyboardOpen: true, visualHeight: 450, offsetTop: 30 });
assert.equal(keyboardViewportState({ rootHeight: 450, layoutHeight: 800, visualHeight: 450, offsetTop: 0 }).keyboardOpen, true);
assert.equal(keyboardViewportState({ rootHeight: 450, layoutHeight: 800, visualHeight: 450, offsetTop: 0 }).inset, 0);
assert.equal(keyboardViewportState({ rootHeight: 450, layoutHeight: 450, visualHeight: 450, offsetTop: 0 }).inset, 0);
assert.equal(keyboardViewportState({ rootHeight: 800, layoutHeight: 800, visualHeight: 720, offsetTop: 0 }).keyboardOpen, false);
assert.equal(keyboardViewportState({ rootHeight: 510, layoutHeight: 844, visualHeight: 450, offsetTop: 0 }).inset, 60);
assert.equal(keyboardViewportState({ rootHeight: 844, layoutHeight: 844, visualHeight: 450, offsetTop: 30 }).inset, 364);
assert.equal(keyboardViewportState({ rootHeight: 844, layoutHeight: 844, visualHeight: 844, offsetTop: 0 }).inset, 0);
console.log("keyboard viewport: 9 checks passed");
