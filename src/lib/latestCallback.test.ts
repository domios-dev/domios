import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { setImmediate } from "node:timers/promises";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { createLatestCallback } from "./latestCallback";

test("forwarder keeps its identity and calls only the latest implementation", () => {
  const slot = { current: (value: number) => value + 1 };
  const callback = createLatestCallback(slot);
  assert.equal(callback(2), 3);
  slot.current = (value) => value * 3;
  assert.equal(callback(2), 6);
});

type Handler = () => unknown;
type Bind = (key: string, handler: Handler, revision: number) => Handler;

function render(messages: object[], revision: number, bind: Bind) {
  const answer: Handler = bind("answer", () => focus, Math.floor(revision / 2));
  const focus: Handler = bind("focus", () => answer, Math.floor((revision + 1) / 2));
  return { answer, focus, rows: () => messages };
}

function fixture(forward: boolean) {
  const slots = new Map<string, { current: Handler; revision: number; callback: Handler }>();
  const bind: Bind = (key, handler, revision) => {
    let slot = slots.get(key);
    if (!slot) {
      slot = { current: handler, revision, callback: handler };
      if (forward) slot.callback = createLatestCallback(slot);
      slots.set(key, slot);
    } else {
      slot.current = handler;
      if (slot.revision !== revision) {
        slot.revision = revision;
        slot.callback = forward ? createLatestCallback(slot) : handler;
      }
    }
    return slot.callback;
  };
  const previous: WeakRef<object[]>[] = [];
  let current: ReturnType<typeof render> | undefined;
  for (let revision = 0; revision < 160; revision++) {
    const messages = Array.from({ length: 32 + revision }, () => ({}));
    previous.push(new WeakRef(messages));
    current = render(messages, revision, bind);
  }
  return { previous: previous.slice(0, -1), current, slots };
}

test("alternating memo lifetimes release previous transcript arrays", async () => {
  if (!global.gc) {
    const child = spawnSync(process.execPath, ["--expose-gc", "--import", "tsx", fileURLToPath(import.meta.url)], {
      encoding: "utf8", timeout: 5000,
    });
    assert.equal(child.status, 0, child.stderr || child.stdout);
    return;
  }
  const legacy = fixture(false);
  const isolated = fixture(true);
  await setImmediate();
  global.gc!();
  assert.ok(legacy.previous.filter((ref) => ref.deref()).length > 100, "control must reproduce the render-context chain");
  assert.equal(isolated.previous.filter((ref) => ref.deref()).length, 0);
  assert.equal(isolated.current!.rows().length, 191);
  assert.equal(legacy.current!.rows().length, 191);
});
