import assert from "node:assert/strict";
import { test } from "node:test";
import { completeResize, createResizeModel, requestResize, setResizeAuthority, type ResizeModel } from "./resizeModel";

const owner = { ptyId: "pty", clientId: "viewer" };
const initial = { cols: 80, rows: 24 };
const last = { cols: 160, rows: 48 };
const owned = () => setResizeAuthority(createResizeModel(), owner, initial);

function finish(state: ResizeModel, succeeded = true) {
  assert.ok(state.inFlight);
  return completeResize(state, state.inFlight, succeeded);
}

test("a burst sends one trailing resize containing only the latest geometry", () => {
  let state = requestResize(owned(), { cols: 90, rows: 30 });
  const first = state.inFlight;
  for (let cols = 91; cols <= 160; cols++) {
    state = requestResize(state, { cols, rows: 48 });
    assert.equal(state.inFlight, first);
    assert.deepEqual(state.applied, initial);
  }
  state = finish(state);
  assert.deepEqual(state.inFlight, { ...owner, ...last, generation: state.generation });
  assert.equal(state.pending, null);
  state = finish(state);
  assert.equal(state.inFlight, null);
  assert.deepEqual(state.applied, last);
});

test("returning to the in-flight geometry removes an obsolete trailing resize", () => {
  let state = requestResize(owned(), last);
  state = requestResize(state, initial);
  state = requestResize(state, last);
  state = finish(state);
  assert.equal(state.inFlight, null);
  assert.deepEqual(state.applied, last);
});

test("returning to the committed geometry still applies it after the in-flight resize", () => {
  let state = requestResize(owned(), last);
  state = requestResize(state, initial);
  state = finish(state);
  assert.equal(state.inFlight?.cols, initial.cols);
  state = finish(state);
  assert.deepEqual(state.applied, initial);
});

test("ownership loss invalidates pending work and stale completion cannot commit", () => {
  let state = requestResize(owned(), { cols: 90, rows: 30 });
  state = requestResize(state, last);
  state = setResizeAuthority(state, null);
  state = finish(state);
  assert.equal(state.inFlight, null);
  assert.equal(state.pending, null);
  assert.equal(state.applied, null);
});

for (const succeeded of [true, false]) {
  test(`a new owner waits for the old operation and survives its ${succeeded ? "success" : "failure"}`, () => {
    let state = requestResize(owned(), last);
    const stale = state.inFlight;
    state = requestResize(state, { cols: 100, rows: 30 });
    state = setResizeAuthority(state, { ...owner, ptyId: "replacement" }, initial);
    state = requestResize(state, initial);
    assert.equal(state.inFlight, stale);
    state = finish(state, succeeded);
    assert.equal(state.inFlight?.ptyId, "replacement");
    assert.equal(state.inFlight?.cols, initial.cols);
    assert.equal(state.applied, null);
    assert.ok(stale);
    assert.equal(completeResize(state, stale, true), state);
    state = finish(state);
    assert.deepEqual(state.applied, initial);
  });
}

test("failure discards pending work and a later explicit request can retry", () => {
  let state = requestResize(owned(), last);
  state = requestResize(state, initial);
  state = finish(state, false);
  assert.equal(state.pending, null);
  assert.equal(state.inFlight, null);
  assert.equal(state.applied, null);
  state = requestResize(state, last);
  assert.ok(state.inFlight);
  assert.deepEqual(finish(state).applied, last);
});

test("duplicate sizes deduplicate unless forced and invalid sizes never start", () => {
  let state = owned();
  assert.equal(requestResize(state, initial).inFlight, null);
  assert.equal(requestResize(state, { cols: 0, rows: 24 }), state);
  state = requestResize(state, initial, true);
  assert.ok(state.inFlight);
  state = requestResize(state, initial, true);
  state = finish(state);
  assert.ok(state.inFlight);
  state = finish(state);
  assert.equal(state.inFlight, null);
  assert.equal(requestResize(createResizeModel(), last).inFlight, null);
});

test("every successful burst ends at the last requested geometry", () => {
  for (let split = 0; split < 16; split++) {
    let state = owned();
    let requested = initial;
    for (let index = 0; index < 16; index++) {
      requested = { cols: 80 + (index % 5), rows: 24 + (index % 3) };
      state = requestResize(state, requested);
      if (index === split && state.inFlight) state = finish(state);
    }
    while (state.inFlight) state = finish(state);
    assert.deepEqual(state.applied, requested);
  }
});
