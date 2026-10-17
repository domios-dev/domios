import assert from "node:assert/strict";
import { createInputQueue } from "./inputQueue";
import type { StreamInput } from "./streamModel";

async function main() {
const pointer = (phase: "down" | "move" | "up", x: number): StreamInput => ({ kind: "pointer", phase, x, y: 5, button: "left", buttons: phase === "up" ? 0 : 1, clickCount: 1, modifiers: 0 });
const calls: StreamInput[] = [];
let complete: (() => void) | undefined;
const queue = createInputQueue(async (input) => {
  calls.push(input);
  if (calls.length === 1) await new Promise<void>((resolve) => { complete = resolve; });
}, (error) => { throw error; });
queue.push(pointer("down", 1));
for (let i = 2; i < 1000; i++) queue.push(pointer("move", i));
queue.push(pointer("up", 1000));
queue.push({ kind: "text", text: "after release" });
const flushed = queue.flush();
assert.equal(calls.length, 1);
complete!();
await flushed;
assert.deepEqual(calls, [pointer("down", 1), pointer("move", 999), pointer("up", 1000), { kind: "text", text: "after release" }]);

const released: StreamInput[] = [];
const cleanup = createInputQueue(async (input) => { released.push(input); }, (error) => { throw error; });
cleanup.push(pointer("down", 3));
cleanup.dispose();
await cleanup.flush();
assert.deepEqual(released, [pointer("down", 3), pointer("up", 3)]);

const errors: unknown[] = [];
let attempts = 0;
const recovery = createInputQueue(async () => { if (++attempts === 1) throw new Error("refused"); }, (error) => errors.push(error));
recovery.push(pointer("down", 1));
recovery.push(pointer("up", 2));
await recovery.flush();
assert.equal(attempts, 2);
assert.equal(errors.length, 1);

const wheels: StreamInput[] = [];
let finishWheel: (() => void) | undefined;
const scroll = createInputQueue(async (input) => {
  wheels.push(input);
  if (wheels.length === 1) await new Promise<void>((resolve) => { finishWheel = resolve; });
}, (error) => { throw error; });
for (let i = 0; i < 100; i++) scroll.push({ kind: "wheel", x: 2, y: 3, deltaX: 0, deltaY: 10 });
finishWheel!();
await scroll.flush();
assert.equal(wheels.length, 2);
assert.equal(wheels.reduce((sum, input) => sum + (input.kind === "wheel" ? input.deltaY : 0), 0), 1000);
console.log("Browser input queue: ordered edges, coalesced movement, cleanup and failure recovery passed");
}
void main().catch((error) => { console.error(error); process.exitCode = 1; });
