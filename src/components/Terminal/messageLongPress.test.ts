import assert from "node:assert/strict";
import { createMessageLongPress } from "./messageLongPress";

function fixture() {
  const pending = new Map<number, () => void>();
  let serial = 0;
  let opened = 0;
  let owned = false;
  let delay = 0;
  const controller = createMessageLongPress({
    schedule: (callback, ms) => { delay = ms; pending.set(++serial, callback); return serial; },
    cancel: (handle) => { pending.delete(handle as number); },
    open: () => { opened++; },
    owned: (value) => { owned = value; },
  });
  return { controller, pending, fire: () => { const callbacks = [...pending.values()]; pending.clear(); for (const callback of callbacks) callback(); }, opened: () => opened, owned: () => owned, delay: () => delay };
}
const point = { pointerId: 1, x: 20, y: 30 };
{
  const f = fixture();
  f.controller.start(point);
  assert.equal(f.delay(), 450);
  assert.equal(f.owned(), true);
  assert.equal(f.opened(), 0);
  f.fire();
  assert.equal(f.opened(), 1);
  assert.equal(f.controller.end(2), false);
  assert.equal(f.controller.end(1), true);
  assert.equal(f.owned(), false);
  assert.equal(f.controller.end(1), false);
}
{
  const f = fixture();
  f.controller.start(point);
  assert.equal(f.controller.end(1), false);
  f.fire();
  assert.equal(f.opened(), 0);
  assert.equal(f.owned(), false);
}
for (const cancel of [
  (f: ReturnType<typeof fixture>) => f.controller.move({ ...point, x: 29 }),
  (f: ReturnType<typeof fixture>) => f.controller.move({ ...point, x: 26, y: 36 }),
  (f: ReturnType<typeof fixture>) => f.controller.cancel(),
]) {
  const f = fixture();
  f.controller.start(point);
  cancel(f);
  f.fire();
  assert.equal(f.opened(), 0);
  assert.equal(f.owned(), false);
}
{
  const f = fixture();
  f.controller.start(point);
  f.controller.move({ ...point, x: 28 });
  f.controller.move({ ...point, pointerId: 2, x: 100 });
  f.fire();
  assert.equal(f.opened(), 1);
}
{
  const f = fixture();
  f.controller.start(point);
  f.controller.start({ ...point, pointerId: 2 });
  assert.equal(f.pending.size, 1);
  f.fire();
  assert.equal(f.opened(), 1);
  f.controller.cancel();
  assert.equal(f.owned(), false);
}
