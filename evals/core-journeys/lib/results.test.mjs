import assert from "node:assert/strict";
import { mkdtempSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { emptyResults, journeySummary, recorder, renderMarkdown, verdict } from "./results.mjs";
import { historyFiller, marker, runId } from "./fixtures.mjs";

test("an empty run is never a pass", () => {
  assert.deepEqual(verdict(emptyResults()), { pass: false, reason: "no checks executed" });
});

test("recorder persists pass and failure with reasons and per-journey time", async () => {
  const out = mkdtempSync(join(tmpdir(), "cj-results-"));
  const results = emptyResults();
  const check = recorder(out, results, { log: () => {} });
  assert.deepEqual(await check("J1", "spawn_ready", "claude", async () => ({ summary: "ok" })), { summary: "ok" });
  assert.equal(await check("J4", "restored_tab_ready", "t1", async () => { throw Object.assign(new Error("x"), { reason: "runtime degraded: SizeLimit" }); }), undefined);
  const saved = JSON.parse(readFileSync(join(out, "results.json"), "utf8"));
  assert.deepEqual(saved.checks.map((entry) => [entry.journey, entry.pass]), [["J1", true], ["J4", false]]);
  assert.equal(saved.checks[1].reason, "runtime degraded: SizeLimit");
  assert.equal(verdict(saved).pass, false);
  assert.deepEqual(Object.keys(journeySummary(saved.checks)), ["J1", "J4"]);
  assert.match(readFileSync(join(out, "results.md"), "utf8"), /SizeLimit/);
});

test("markdown escapes table separators", () => {
  const results = { ...emptyResults(), checks: [{ journey: "J2", name: "ui", subject: "a|b", pass: true, ms: 1 }] };
  assert.match(renderMarkdown(results), /a\\\|b/);
});

test("fixtures: filler has the exact size, markers are alphanumeric and run ids unique", () => {
  assert.equal(historyFiller(327_680).length, 327_680);
  assert.equal(historyFiller(0), "");
  assert.match(marker("ab1", "open-code", "h"), /^CJAB1OPENCODEH$/);
  assert.notEqual(runId(1, 0.1), runId(1, 0.9));
});
