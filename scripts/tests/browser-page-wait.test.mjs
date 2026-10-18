import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import vm from "node:vm";

const source = readFileSync(new URL("../../src-tauri/crates/codeterm-surface/src/surface/browser/wait.js", import.meta.url), "utf8");

function fixture() {
  const observers = new Set();
  const timers = new Map();
  let selected = false;
  let selectorError;
  const document = {
    body: { innerText: "initial" },
    querySelector() {
      if (selectorError) throw selectorError;
      return selected ? {} : null;
    },
  };
  const wait = vm.runInNewContext(source, {
    document,
    MutationObserver: class {
      constructor(callback) { this.callback = callback; }
      observe() { observers.add(this); }
      disconnect() { observers.delete(this); }
    },
    setTimeout(callback) { const id = Symbol(); timers.set(id, callback); return id; },
    clearTimeout(id) { timers.delete(id); },
  });
  return {
    wait, document, observers, timers,
    select() { selected = true; },
    invalidSelector() { selectorError = new Error("invalid selector"); },
    mutate() { for (const observer of [...observers]) observer.callback(); },
    expire() { for (const callback of [...timers.values()]) callback(); },
  };
}

test("existing text resolves without allocating a timer or observer", async () => {
  const f = fixture();
  assert.equal(await f.wait("text", "initial", 15000), true);
  assert.equal(f.observers.size, 0);
  assert.equal(f.timers.size, 0);
});

test("a text mutation resolves and releases its observer and deadline", async () => {
  const f = fixture();
  const result = f.wait("text", "done", 15000);
  assert.equal(f.observers.size, 1);
  assert.equal(f.timers.size, 1);
  f.document.body.innerText = "done";
  f.mutate();
  assert.equal(await result, true);
  assert.equal(f.observers.size, 0);
  assert.equal(f.timers.size, 0);
});

test("a selector resolves from a DOM event", async () => {
  const f = fixture();
  const result = f.wait("selector", "#ready", 15000);
  f.select();
  f.mutate();
  assert.equal(await result, true);
  assert.equal(f.observers.size, 0);
  assert.equal(f.timers.size, 0);
});

test("a missing body can become available without polling", async () => {
  const f = fixture();
  f.document.body = null;
  const result = f.wait("text", "ready", 15000);
  f.document.body = { innerText: "ready" };
  f.mutate();
  assert.equal(await result, true);
  assert.equal(f.timers.size, 0);
});

test("expiry rejects and releases resources instead of implying readiness", async () => {
  const f = fixture();
  const rejection = assert.rejects(f.wait("text", "never", 15000), /deadline exceeded/);
  f.expire();
  await rejection;
  assert.equal(f.observers.size, 0);
  assert.equal(f.timers.size, 0);
});

test("invalid selectors reject without leaving pending work", async () => {
  const f = fixture();
  f.invalidSelector();
  await assert.rejects(f.wait("selector", "[", 15000), /invalid selector/);
  assert.equal(f.observers.size, 0);
  assert.equal(f.timers.size, 0);
});
