import assert from "node:assert/strict";
import test from "node:test";
import { formatPerfFailureState, PerfEventBounce, type PerfEventRow } from "./perf-event-bounce";

test("event bounce uses a receiver-safe default clock", () => {
  const rows: PerfEventRow[] = [];
  const startedAt = performance.now();
  const bounce = new PerfEventBounce({ run: "run-default-clock", test: "pty-write-to-render", index: 7, startedAt }, (row) => rows.push(row));

  const row = bounce.record("sent");
  const elapsedMs = bounce.elapsedMs();

  assert.equal(row?.phase, "sent");
  assert.equal(rows[0]?.phase, "sent");
  assert.ok(rows[0].t_ms >= 0);
  assert.ok(elapsedMs >= 0);
});

test("event bounce is append-only with one row per lifecycle phase", () => {
  const rows: PerfEventRow[] = [];
  let now = 100;
  const bounce = new PerfEventBounce({ run: "run-1", test: "pty-write-to-render", index: 4, startedAt: 100 }, (row) => rows.push(row), () => now);

  bounce.record("sent");
  now = 140;
  bounce.record("output", { marker: "PWRENDER" });
  now = 160;
  bounce.record("render", { visible: true });
  assert.equal(bounce.record("render", { visible: true }), null);

  assert.deepEqual(rows.map(({ run, test: testName, index, seq, phase, t_ms }) => ({ run, test: testName, index, seq, phase, t_ms })), [
    { run: "run-1", test: "pty-write-to-render", index: 4, seq: 0, phase: "sent", t_ms: 0 },
    { run: "run-1", test: "pty-write-to-render", index: 4, seq: 1, phase: "output", t_ms: 40 },
    { run: "run-1", test: "pty-write-to-render", index: 4, seq: 2, phase: "render", t_ms: 60 },
  ]);
});

test("event bounce retains a real output error without substituting output absence", () => {
  const rows: PerfEventRow[] = [];
  const bounce = new PerfEventBounce({ run: "run-1", test: "pty-write-to-render", index: 0, startedAt: 0 }, (row) => rows.push(row), () => 10);

  bounce.record("sent");
  bounce.record("output-error", { error: "GET /panes/pane-1/text: socket closed" });

  assert.equal(rows[1].phase, "output-error");
  assert.equal(rows[1].error, "GET /panes/pane-1/text: socket closed");
  assert.equal(rows.some((row) => row.phase === "output"), false);
});

test("event bounce preserves an explicit first-observation timestamp", () => {
  const rows: PerfEventRow[] = [];
  const bounce = new PerfEventBounce({ run: "run-1", test: "pty-write-to-render", index: 0, startedAt: 0 }, (row) => rows.push(row), () => 900);

  bounce.record("render", { visible: true }, 412);

  assert.equal(rows[0].t_ms, 412);
});

test("render event carries the full focus and xterm viewport observation", () => {
  const rows: PerfEventRow[] = [];
  const focus = { rootMounted: true, paneVisible: true, tabActive: true, terminalMode: true };
  const viewport = { scrollTop: 100, scrollHeight: 200, clientHeight: 100, bottomDelta: 0, anchored: true };
  const bounce = new PerfEventBounce({ run: "run-1", test: "pty-write-to-render", index: 0, startedAt: 0 }, (row) => rows.push(row), () => 500);

  bounce.record("render", { focus, viewport }, 412);

  assert.deepEqual(rows[0].focus, focus);
  assert.deepEqual(rows[0].viewport, viewport);
  assert.equal(rows[0].t_ms, 412);
});

test("late capture discoveries append output and render before capture", () => {
  const rows: PerfEventRow[] = [];
  const bounce = new PerfEventBounce({ run: "run-1", test: "pty-write-to-render", index: 0, startedAt: 0 }, (row) => rows.push(row), () => 4600);

  bounce.record("sent");
  bounce.record("output", { late: true });
  bounce.record("render", { late: true });
  bounce.record("capture", { outputPath: "/tmp/results/diag.png" });

  assert.deepEqual(rows.map((row) => row.phase), ["sent", "output", "render", "capture"]);
  assert.equal(rows[3].outputPath, "/tmp/results/diag.png");
});

function failureBounce() {
  return new PerfEventBounce({ run: "run-1", test: "pty-write-to-render", index: 0, startedAt: 0 }, () => {}, () => 5000);
}

test("failure formatter prioritizes a real output error over output absence", () => {
  const bounce = failureBounce();
  bounce.record("output-error", { error: "socket closed" });
  assert.equal(formatPerfFailureState(bounce, 4500), "(b) output-error: socket closed render>4500ms");
});

test("failure formatter reports output absence without an API error", () => {
  assert.equal(formatPerfFailureState(failureBounce(), 4500), "(b) output-absent: render>4500ms");
});

test("failure formatter reports a late render after output", () => {
  const bounce = failureBounce();
  bounce.record("output", {}, 812);
  bounce.record("render", { late: true }, 5200);
  assert.equal(formatPerfFailureState(bounce, 4500), "(c) render-late: output@812ms render@5200ms>budget4500ms");
});

test("failure formatter reports focus, view, and bottom mismatches", () => {
  const bounce = failureBounce();
  bounce.record("output", {}, 812);
  bounce.record("capture", { focus: { rootMounted: true, paneVisible: true, tabActive: false, terminalMode: true }, viewport: { anchored: false, bottomDelta: 24 } });
  assert.equal(formatPerfFailureState(bounce, 4500), "(a) focus-view-bottom-mismatch: output@812ms render>4500ms; tab-inactive,bottomDelta=24");
});

test("failure formatter reports the focused render pipeline and retains transient errors", () => {
  const bounce = failureBounce();
  bounce.record("output-error", { error: "temporary socket reset" });
  bounce.record("output", {}, 812);
  bounce.record("capture", { focus: { rootMounted: true, paneVisible: true, tabActive: true, terminalMode: true }, viewport: { anchored: true, bottomDelta: 0 } });
  assert.equal(formatPerfFailureState(bounce, 4500), "(a) render-pipeline: output@812ms render>4500ms; output-error: temporary socket reset");
});
