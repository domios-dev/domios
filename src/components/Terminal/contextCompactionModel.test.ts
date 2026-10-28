import assert from "node:assert/strict";
import { test } from "node:test";
import type { NormalizedChatMessage } from "@codeterm/chat-core";
import { reduceToolEvents } from "@codeterm/chat-core";
import { emptyCompactionHistory, observeCompaction, projectCompactionRows, reconcileCompactionRows, type CompactionHistory } from "./contextCompactionModel";
import { createCompactionStore, type CompactionPersistence } from "./contextCompactionStore";

const start = { phase: "compacting" as const, at: 1_800_000_000_000, id: "compact-1", afterId: "a" };
const rows: NormalizedChatMessage[] = [
  { id: "a", type: "assistant", content: "opaque first" },
  { id: "b", type: "assistant", content: "opaque second" },
];

test("compaction starts once, remains last while running, and settles at its last anchor", () => {
  const started = observeCompaction(emptyCompactionHistory, start);
  assert.equal(observeCompaction(started, { ...start, id: "duplicate" }), started);
  assert.deepEqual(projectCompactionRows(rows, started).map((row) => row.id), ["a", "b", "compact-1"]);
  const advanced = observeCompaction(started, { ...start, afterId: "b" });
  const finished = observeCompaction(advanced, { ...start, phase: "other", at: start.at + 2000, afterId: "b" });
  assert.deepEqual(finished.rows[0], { id: "compact-1", afterId: "b", startedAt: start.at, endedAt: start.at + 2000, state: "completed" });
  assert.deepEqual(projectCompactionRows([...rows, { id: "c", type: "assistant", content: "later" }], finished).map((row) => row.id), ["a", "b", "compact-1", "c"]);
});

test("unknown state does not end compaction, errors do, and repeated failures are inert", () => {
  const started = observeCompaction(emptyCompactionHistory, start);
  assert.equal(observeCompaction(started, { ...start, phase: "unknown" }), started);
  const failed = observeCompaction(started, { ...start, phase: "failed", at: start.at + 20 });
  assert.equal(failed.rows[0].state, "failed");
  assert.equal(observeCompaction(failed, { ...start, phase: "failed" }), failed);
  assert.equal(reconcileCompactionRows(failed, [{ id: "provider", type: "system", content: "", timelineMarker: { kind: "context_compaction" } }], start.at + 30), failed);
});

test("provider completion replaces its running row once without moving its identity", () => {
  const started = observeCompaction(emptyCompactionHistory, start);
  const messages: NormalizedChatMessage[] = [rows[0], { id: "provider", type: "system", content: "", timelineMarker: { kind: "context_compaction" } }, rows[1]];
  const completed = reconcileCompactionRows(started, messages, start.at + 500);
  assert.equal(completed.rows[0].state, "completed");
  assert.equal(completed.rows[0].providerRowId, "provider");
  assert.deepEqual(projectCompactionRows(messages, completed).map((row) => row.id), ["a", "compact-1", "b"]);
  assert.equal(reconcileCompactionRows(completed, messages, start.at + 900), completed);
  assert.equal(observeCompaction(completed, start), completed);
});

test("reload deduplicates active and completed rows before accepting phase observations", async () => {
  const saved = new Map<string, CompactionHistory>();
  const persistence: CompactionPersistence = {
    async load(key) { return structuredClone(saved.get(key) ?? emptyCompactionHistory); },
    async save(key, next) { saved.set(key, structuredClone(next)); },
  };
  const first = createCompactionStore(persistence, () => start.at, () => "stable");
  await first.observe("own-session", "compacting", "a");
  const restored = createCompactionStore(persistence, () => start.at + 100, () => "duplicate");
  await restored.observe("own-session", "compacting", "a");
  assert.deepEqual(restored.get("own-session").rows.map((row) => row.id), ["stable"]);
  await restored.observe("own-session", "other", "a");
  const reopened = createCompactionStore(persistence);
  await reopened.hydrate("own-session");
  assert.equal(reopened.get("own-session").rows.length, 1);
  assert.equal(reopened.get("own-session").rows[0].state, "completed");
  await reopened.hydrate("foreign-session");
  assert.equal(reopened.get("foreign-session").rows.length, 0);
});

test("a runtime error after provider completion still fails the active compaction", () => {
  const started = observeCompaction(emptyCompactionHistory, start);
  const completed = reconcileCompactionRows(started, [rows[0], { id: "provider", type: "system", content: "", timelineMarker: { kind: "context_compaction" } }], start.at + 30);
  const failed = observeCompaction(completed, { ...start, phase: "failed", at: start.at + 40 });
  assert.equal(failed.rows[0].state, "failed");
  assert.equal(failed.rows[0].providerRowId, "provider");
});

test("distinct compactions remain distinct even without intervening messages", () => {
  let history: CompactionHistory = observeCompaction(emptyCompactionHistory, start);
  history = observeCompaction(history, { ...start, phase: "other", at: start.at + 10 });
  history = observeCompaction(history, { ...start, id: "compact-2", at: start.at + 20 });
  const projected = projectCompactionRows([rows[0]], history);
  assert.deepEqual(projected.map((row) => row.id), ["a", "compact-1", "compact-2"]);
  assert.equal(reduceToolEvents(projected).length, 3);
});

test("a paged window cannot move a historical marker into an unrelated older page", () => {
  const history = observeCompaction(observeCompaction(emptyCompactionHistory, start), { ...start, phase: "other" });
  const old: NormalizedChatMessage[] = [{ id: "old", type: "assistant", content: "old", timestamp: String(start.at - 10000) }];
  assert.deepEqual(projectCompactionRows(old, history), old);
});
