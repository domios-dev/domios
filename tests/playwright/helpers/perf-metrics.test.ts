import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { PerfEventBounce } from "./perf-event-bounce";
import { recordPerfEvent, recordPerfMetric, recordPerfSamples, summarizePerfSamples } from "./perf-metrics";

test("summarizePerfSamples emits nearest-rank p50, p95, and max", () => {
  assert.deepEqual(summarizePerfSamples([90, 10, 70, 50, 30]), {
    n: 5,
    p50: 50,
    p95: 90,
    max: 90,
  });
});

test("summarizePerfSamples rejects empty and non-finite samples", () => {
  assert.throws(() => summarizePerfSamples([]), /at least one sample/);
  assert.throws(() => summarizePerfSamples([1, Number.NaN]), /finite/);
});

test("recordPerfSamples writes a deterministic summary row", () => {
  const dir = mkdtempSync(join(tmpdir(), "codeterm-perf-metrics-"));
  const previous = process.env.PERF_METRICS_DIR;
  process.env.PERF_METRICS_DIR = dir;
  try {
    recordPerfSamples({
      name: "visible-write-render",
      unit: "ms",
      samples: [12, 4, 8],
      budget: 20,
      mode: "stub",
    });
    const rows = readFileSync(join(dir, "metrics.jsonl"), "utf8").trim().split("\n");
    assert.equal(rows.length, 1);
    assert.deepEqual(JSON.parse(rows[0]), {
      name: "visible-write-render",
      unit: "ms",
      n: 3,
      p50: 8,
      p95: 12,
      max: 12,
      budget: 20,
      mode: "stub",
    });
  } finally {
    if (previous === undefined) delete process.env.PERF_METRICS_DIR;
    else process.env.PERF_METRICS_DIR = previous;
    rmSync(dir, { recursive: true, force: true });
  }
});

test("recordPerfMetric keeps single-value callers in the summary schema", () => {
  const dir = mkdtempSync(join(tmpdir(), "codeterm-perf-metric-"));
  const previous = process.env.PERF_METRICS_DIR;
  process.env.PERF_METRICS_DIR = dir;
  try {
    recordPerfMetric({ name: "single", unit: "ms", value: 9, budget: 10 });
    assert.deepEqual(JSON.parse(readFileSync(join(dir, "metrics.jsonl"), "utf8")), {
      name: "single",
      unit: "ms",
      n: 1,
      p50: 9,
      p95: 9,
      max: 9,
      budget: 10,
    });
  } finally {
    if (previous === undefined) delete process.env.PERF_METRICS_DIR;
    else process.env.PERF_METRICS_DIR = previous;
    rmSync(dir, { recursive: true, force: true });
  }
});

test("sent is synchronously persisted before an interrupted sample can emit later phases", () => {
  const dir = mkdtempSync(join(tmpdir(), "codeterm-perf-event-"));
  const previous = process.env.PERF_METRICS_DIR;
  process.env.PERF_METRICS_DIR = dir;
  try {
    const bounce = new PerfEventBounce(
      { run: "run-1", test: "pty-write-to-render", index: 2, startedAt: 100 },
      recordPerfEvent,
      () => 100,
    );
    bounce.record("sent", { marker: "PWRENDER" });
    const rows = readFileSync(join(dir, "metrics.jsonl"), "utf8").trim().split("\n").map((row) => JSON.parse(row));
    assert.deepEqual(rows, [{ run: "run-1", test: "pty-write-to-render", index: 2, seq: 0, phase: "sent", t_ms: 0, marker: "PWRENDER" }]);
  } finally {
    if (previous === undefined) delete process.env.PERF_METRICS_DIR;
    else process.env.PERF_METRICS_DIR = previous;
    rmSync(dir, { recursive: true, force: true });
  }
});
