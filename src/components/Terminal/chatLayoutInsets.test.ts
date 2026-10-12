import assert from "node:assert/strict";
import { chatFloatingBottomInset } from "./chatLayoutInsets";

assert.equal(chatFloatingBottomInset(500, []), 0);
assert.equal(chatFloatingBottomInset(500, [458]), 42);
assert.equal(chatFloatingBottomInset(500, [458, 450]), 50);
assert.equal(chatFloatingBottomInset(500, [510]), 0);
assert.equal(chatFloatingBottomInset(500, [457.5]), 43);
console.log("chat floating inset: 5 checks passed");
