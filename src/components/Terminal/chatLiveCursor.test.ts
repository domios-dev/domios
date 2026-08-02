import { strict as assert } from "node:assert";
import { advanceChatCursor, classifyChatCursor } from "./chatLiveCursor";

assert.equal(classifyChatCursor(100, 100, 140), "accept");
assert.equal(classifyChatCursor(100, 140, 180), "gap");
assert.equal(classifyChatCursor(180, 100, 140), "stale");
assert.equal(classifyChatCursor(120, 100, 140), "accept");
assert.equal(classifyChatCursor(null, 100, 140), "accept");
assert.equal(advanceChatCursor(140, 120), 140);
assert.equal(advanceChatCursor(140, 180), 180);
