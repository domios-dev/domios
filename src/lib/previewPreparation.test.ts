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
