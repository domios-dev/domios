import assert from "node:assert/strict";
import test from "node:test";
import {
  classifyDiagnostic,
  computeAnchored,
  diagnosticRow,
  formatDiagnosticMessage,
  type FocusViewState,
  type SampleDiagnostic,
} from "./perf-diag";

const FOCUSED: FocusViewState = { rootMounted: true, paneVisible: true, tabActive: true, terminalMode: true };
const ANCHORED = { scrollTop: 100, scrollHeight: 200, clientHeight: 100 };
const anchoredViewport = () => {
  const g = computeAnchored(ANCHORED.scrollTop, ANCHORED.scrollHeight, ANCHORED.clientHeight);
  return { ...ANCHORED, ...g };
};

function sample(over: Partial<SampleDiagnostic>): SampleDiagnostic {
  return {
    index: 0,
    marker: "PWRENDER1",
    tOutputMs: 800,
    tRenderMs: null,
    budgetMs: 4500,
    loadavg: [1, 1, 1],
    focus: FOCUSED,
    viewport: anchoredViewport(),
    ...over,
  };
}

test("computeAnchored derives bottomDelta and anchored within threshold", () => {
  assert.deepEqual(computeAnchored(100, 200, 100), { bottomDelta: 0, anchored: true });
  assert.deepEqual(computeAnchored(100, 200, 98), { bottomDelta: 2, anchored: true });
  assert.deepEqual(computeAnchored(50, 200, 100), { bottomDelta: 50, anchored: false });
  assert.deepEqual(computeAnchored(0, 80, 100), { bottomDelta: -20, anchored: true });
});

test("class (b): output-absent -> not-emitted", () => {
  const c = classifyDiagnostic(sample({ tOutputMs: null, tRenderMs: null }));
  assert.equal(c.verdict, "(b)");
  assert.equal(c.phase, "output-absent");
  assert.match(c.detail, /output-absent/);
});

test("class (a): output-present but not bottom-anchored -> view/bottom mismatch with delta", () => {
  const notAnchored = { scrollTop: 50, scrollHeight: 200, clientHeight: 100 };
  const g = computeAnchored(notAnchored.scrollTop, notAnchored.scrollHeight, notAnchored.clientHeight);
  const c = classifyDiagnostic(sample({ tOutputMs: 800, tRenderMs: null, viewport: { ...notAnchored, ...g } }));
  assert.equal(c.verdict, "(a)");
  assert.equal(c.phase, "focus-view-bottom-mismatch");
  assert.match(c.detail, /not-bottom-anchored\(delta=50\)/);
  assert.match(c.detail, /output@800ms/);
});

test("class (a): output-present but pane inactive -> focus mismatch", () => {
  const c = classifyDiagnostic(sample({ tOutputMs: 800, tRenderMs: null, focus: { ...FOCUSED, tabActive: false } }));
  assert.equal(c.verdict, "(a)");
  assert.equal(c.phase, "focus-view-bottom-mismatch");
  assert.match(c.detail, /tab-inactive/);
});

test("class (a): output-present, focused+anchored, render-absent -> render-pipeline", () => {
  const c = classifyDiagnostic(sample({ tOutputMs: 812, tRenderMs: null }));
  assert.equal(c.verdict, "(a)");
  assert.equal(c.phase, "render-pipeline");
  assert.equal(c.detail, "output@812ms render>4500ms");
});

test("class (c): render-late -> after budget", () => {
  const c = classifyDiagnostic(sample({ tOutputMs: 800, tRenderMs: 5200 }));
  assert.equal(c.verdict, "(c)");
  assert.equal(c.phase, "render-late");
  assert.match(c.detail, /output@800ms render@5200ms>budget4500ms/);
});

test("ok: rendered within budget", () => {
  const c = classifyDiagnostic(sample({ tOutputMs: 200, tRenderMs: 900 }));
  assert.equal(c.verdict, "ok");
  assert.equal(c.phase, "ok");
});

test("formatDiagnosticMessage names phase and values (output@..ms render>..ms)", () => {
  assert.equal(formatDiagnosticMessage(sample({ tOutputMs: 812, tRenderMs: null })), "(a) render-pipeline: output@812ms render>4500ms");
  assert.equal(formatDiagnosticMessage(sample({ tOutputMs: null, tRenderMs: null })), "(b) output-absent: output-absent");
});

test("diagnosticRow carries ordered index + per-sample series fields", () => {
  const row = diagnosticRow("pty-write-to-render", sample({ index: 3, tOutputMs: 812, tRenderMs: null }), "stub");
  assert.equal(row.type, "diagnostic");
  assert.equal(row.index, 3);
  assert.equal(row.name, "pty-write-to-render");
  assert.equal(row.t_output_ms, 812);
  assert.equal(row.t_render_ms, null);
  assert.equal(row.budget, 4500);
  assert.deepEqual(row.loadavg, [1, 1, 1]);
  assert.equal(row.mode, "stub");
  assert.equal(row.viewport?.bottomDelta, 0);
  assert.equal(row.viewport?.anchored, true);
  assert.equal(row.verdict, "(a)");
});

test("ordered series is reconstructable: monotonic growth vs fifth-cliff", () => {
  const monotonic = [500, 900, 1400, 2000, 2600].map((t, i) => diagnosticRow("pty-write-to-render", sample({ index: i, tOutputMs: 100, tRenderMs: t }), "stub"));
  const cliff = [300, 320, 310, 305, null].map((t, i) => diagnosticRow("pty-write-to-render", sample({ index: i, tOutputMs: 100, tRenderMs: t }), "stub"));
  assert.deepEqual(monotonic.map((r) => r.index), [0, 1, 2, 3, 4]);
  const growth = monotonic.map((r) => r.t_render_ms as number);
  assert.ok(growth.every((v, i) => i === 0 || v > growth[i - 1]), "monotonic series must be strictly increasing");
  assert.equal(cliff[4].t_render_ms, null, "fifth-cliff: last sample never rendered");
  assert.ok((cliff[3].t_render_ms as number) < 400, "fifth-cliff: first four are fast");
});
