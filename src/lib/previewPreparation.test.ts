import assert from "node:assert/strict";
import test from "node:test";
import { createPreviewPreparation, type PreviewPreparation } from "./previewPreparation";

test("cards share one prepare and wait without fetching until their exact index is ready", async () => {
  let prepareCalls = 0;
  let removeCalls = 0;
  let receive!: (state: PreviewPreparation) => void;
  const prepare = createPreviewPreparation({
    prepare: async () => { prepareCalls++; return { state: "indexing", index_id: "own-index" }; },
    listen: async (_tab, handler) => { receive = handler; return () => { removeCalls++; }; },
  });
  let fetched = 0;
  const cards = Array.from({ length: 12 }, () => prepare("own-tab").then(() => { fetched++; }));
  await Promise.resolve(); await Promise.resolve();
  assert.equal(prepareCalls, 1);
  assert.equal(fetched, 0);
  receive({ state: "ready", index_id: "foreign-or-old-index" });
  await Promise.resolve();
  assert.equal(fetched, 0);
  receive({ state: "ready", index_id: "own-index" });
  await Promise.all(cards);
  assert.equal(fetched, 12);
  assert.equal(removeCalls, 1);
});

test("completion before prepare response is retained and failures release subscriptions", async () => {
  let receive!: (state: PreviewPreparation) => void;
  let removed = 0;
  const early = createPreviewPreparation({
    prepare: async () => { receive({ state: "ready", index_id: "early" }); return { state: "indexing", index_id: "early" }; },
    listen: async (_tab, handler) => { receive = handler; return () => { removed++; }; },
  });
  await early("tab");
  assert.equal(removed, 1);
  const failure = createPreviewPreparation({
    prepare: async () => { receive({ state: "failed", index_id: "failed" }); return { state: "indexing", index_id: "failed" }; },
    listen: async (_tab, handler) => { receive = handler; return () => { removed++; }; },
  });
  await assert.rejects(failure("tab"), /indexing failed/);
  assert.equal(removed, 2);
});

test("without an event stream, long-polls only when the peer proves it waited and follows newer index ids", async () => {
  let prepareCalls = 0;
  let removed = 0;
  const prepare = createPreviewPreparation({
    prepare: async () => {
      prepareCalls++;
      if (prepareCalls === 1) return { state: "indexing", index_id: "remote-index-1", waited: true };
      if (prepareCalls === 2) return { state: "indexing", index_id: "remote-index-2", waited: true };
      return { state: "ready", index_id: "remote-index-2", waited: true };
    },
    listen: async () => () => { removed++; },
    canReceiveEvents: () => false,
  });
  await Promise.all([prepare("mesh:peer-a/tab-1"), prepare("mesh:peer-a/tab-1")]);
  assert.equal(prepareCalls, 3);
  assert.equal(removed, 1);
});

test("against a pre-lane peer that does not wait, makes one immediate retry and then fails instead of tight-looping", async () => {
  let slowCalls = 0;
  const slowPeer = createPreviewPreparation({
    prepare: async () => {
      slowCalls++;
      return { state: "indexing", index_id: "legacy-index" };
    },
    listen: async () => () => {},
    canReceiveEvents: () => false,
  });
  await assert.rejects(slowPeer("mesh:legacy/tab-1"), (error: Error & { code?: string }) => {
    assert.equal(error.code, "version_unsupported");
    assert.match(error.message, /updating the peer/i);
    return true;
  });
  assert.equal(slowCalls, 2);

  let fastCalls = 0;
  const fastPeer = createPreviewPreparation({
    prepare: async () => {
      fastCalls++;
      return { state: fastCalls === 1 ? "indexing" : "ready", index_id: "legacy-fast" };
    },
    listen: async () => () => {},
    canReceiveEvents: () => false,
  });
  await fastPeer("mesh:legacy/tab-2");
  assert.equal(fastCalls, 2);
});

test("reconnect follows a newer index id on the same tab instead of throwing", async () => {
  let prepareCalls = 0;
  let receive!: (state: PreviewPreparation) => void;
  let triggerReconnect!: () => void;
  const prepare = createPreviewPreparation({
    prepare: async () => {
      prepareCalls++;
      return { state: "indexing", index_id: prepareCalls === 1 ? "run-1" : "run-2" };
    },
    listen: async (_tab, handler) => { receive = handler; return () => {}; },
    reconnect: handler => { triggerReconnect = handler; return () => {}; },
  });
  const pending = prepare("own-tab");
  await Promise.resolve(); await Promise.resolve();
  triggerReconnect();
  await Promise.resolve(); await Promise.resolve();
  receive({ state: "ready", index_id: "run-2" });
  await pending;
  assert.equal(prepareCalls, 2);
});
