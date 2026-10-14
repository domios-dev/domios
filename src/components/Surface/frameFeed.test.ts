// Run: npx tsx src/components/Surface/frameFeed.test.ts

import assert from "node:assert/strict";
import { createFrameFeed, frameBytes } from "./frameFeed";
import type { StreamFrame } from "./surfaceStreamTransport";

const frame = (data: StreamFrame["data"]): StreamFrame => ({ data, metadata: { deviceWidth: 1, deviceHeight: 1 } });

const feed = createFrameFeed();
const seen: StreamFrame[] = [];
const unsubscribe = feed.subscribe((next) => seen.push(next));
const first = frame("AA==");
feed.publish(first);
unsubscribe();
feed.publish(frame("AQ=="));
assert.deepEqual(seen, [first], "listeners receive frames until they unsubscribe");

assert.deepEqual([...frameBytes(btoa("\x01\x02"))], [1, 2], "base64 frames decode to bytes");
assert.deepEqual([...frameBytes(new Uint8Array([3, 4]).buffer)], [3, 4], "binary frames pass through");

console.log("✓ frameFeed tests green");
const resumed: StreamFrame[] = [];
const stopResumed = feed.subscribe((next) => resumed.push(next));
assert.deepEqual(resumed, [frame("AQ==")], "returning viewers get the current frame without waiting for a repaint");
stopResumed();
