import assert from "node:assert/strict";
import {
  dreamCounters, dreamHeatmap, dreamNights, dreamRunData, dreamTotals, nightState, selectDreamNight,
  type DreamDiaryEntry, type DreamWorker,
} from "./dreamModel";

const at = new Date(2026, 10, 24, 7).getTime();
const entry = (id: string, body = "<insight>A remembered connection.</insight>"): DreamDiaryEntry => ({
  id, body, title: id, created_at: new Date(at).toISOString(), area: "_dreams",
});
const worker = (id: string, extra: Partial<DreamWorker> = {}): DreamWorker => ({
  worker_id: id, tab_id: null, intent: "dreaming:all", spawned_by: "cron", spawned_at: at,
  status: "done", progress_text: null, touched_nodes: [], color: "", report: null, done_at: at + 60_000, ...extra,
});

const counters = dreamCounters(["<insight>Observed.</insight><log>", "ISLANDS: bridged=3 promoted=2 archived=1",
  "ORPHANS: bridged=4 archived=2", "DEDUP: aliases=1 duplicates=2", "CALIBRATION: skipped=none applied=17 archived=1",
  "PARALLEL_EDGES: collapsed=745", "DREAM_BACKLINKS: 51", "</log>"].join("\n"));
assert.deepEqual(counters, { links: 7, merged: 3, archived: 4, reweighted: 17, collapsed: 745, backlinks: 51 });
assert.equal(dreamCounters("<log>CALIBRATION: pending\nISLANDS: bridged=pending</log>").links, 0);
assert.equal(dreamCounters("<insight>There were 123 memories.</insight>").links, 0);

assert.equal(nightState(entry("good")), "complete");
assert.equal(nightState(entry("wip", "<insight>WIP: dreaming</insight>")), "incomplete");
assert.equal(nightState(entry("pending", "<insight>Read.</insight><log>CALIBRATION: pending</log>")), "incomplete");
assert.equal(nightState(undefined, worker("running", { status: "running" })), "live");
assert.equal(nightState(undefined, worker("failed", { status: "error" })), "stopped");
assert.equal(nightState(entry("written"), worker("failed", { finalization: { state: "failed", reason: "stopped" } })), "stopped");
assert.equal(nightState(undefined, worker("unverified")), "incomplete");

const nights = dreamNights([entry("diary")], [
  worker("own", { diary_id: "diary" }),
  worker("stopped", { status: "error", spawned_at: at + 10 }),
  worker("other", { diary_id: "another", spawned_at: at - 10 }),
]);
assert.equal(nights.length, 3);
assert.equal(nights.find((night) => night.id === "diary")?.worker?.worker_id, "own");
assert.equal(nights[0].id, "stopped");
assert.equal(selectDreamNight(nights, null)?.id, "diary");
assert.equal(selectDreamNight(nights, "stopped")?.id, "stopped");
assert.equal(selectDreamNight(nights, "removed")?.id, "diary");
assert.equal(selectDreamNight([], null), undefined);

const cells = dreamHeatmap(nights, at);
assert.equal(cells.length, 126);
assert.equal(new Date(cells[0].at).getDay(), 0);
assert.equal(cells.filter((cell) => cell.today).length, 1);
assert.equal(cells.find((cell) => cell.today)?.nights.length, 3);
assert.equal(cells.filter((cell) => cell.future).length, 4);
for (let index = 1; index < cells.length; index++) {
  const expected = new Date(cells[index - 1].at);
  expected.setDate(expected.getDate() + 1);
  assert.equal(cells[index].at, +expected, "calendar days survive daylight saving transitions");
}
const withCounters = nights.map((night) => ({ ...night, counters }));
const outside = { ...withCounters[0], at: cells[0].at - 1 };
assert.deepEqual(dreamTotals([...withCounters, outside], at), { nights: 1, links: 21, merged: 9, stopped: 1 });

assert.deepEqual(dreamRunData(worker("old")), { ids: [], edges: [], actions: [], hasConstellation: false, hasItems: false });
const real = dreamRunData(worker("new", {
  touched_nodes: ["a", "b", "a"],
  node_actions: { a: "linked", missing: "archived" },
  created_edges: [{ from: "a", to: "b", relation: "uses" }, { from: "a", to: "missing", relation: "uses" }],
}));
assert.deepEqual(real.ids, ["a", "b"]);
assert.equal(real.edges.length, 1);
assert.deepEqual(real.actions, [["a", "linked"]]);
assert.equal(real.hasConstellation, true);
assert.equal(real.hasItems, true);
assert.equal(dreamRunData(worker("actions-only", { node_actions: { a: "merged" } })).hasItems, false);
console.log("✓ dream model: counters, night identity/state, calendar heatmap, totals and real-data fallback");

const { dreamRefresh } = await import("./dreamRefresh");
let release: (() => void) | undefined;
let calls = 0;
let active = true;
const refresh = dreamRefresh(async () => {
  calls++;
  if (calls === 1) await new Promise<void>((resolve) => { release = resolve; });
}, () => active);
const first = refresh();
await refresh();
await refresh();
assert.equal(calls, 1, "a burst never overlaps loads");
release!();
await first;
assert.equal(calls, 2, "one trailing load consumes all invalidations");
active = false;
console.log("✓ dream refresh serializes bursts and preserves one trailing invalidation");
