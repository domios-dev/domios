/**
 * Perf gate metrics sink: every perf test appends one JSON line per metric to
 * artifacts/perf/metrics.jsonl (p50/p95/max or a single value), giving shard
 * machines a machine-readable trend artifact next to the hard asserts.
 */
import { appendFileSync, mkdirSync } from "node:fs";
import path from "node:path";
import type { PerfEventRow } from "./perf-event-bounce";

const defaultPerfDir = path.resolve(__dirname, "..", "artifacts", "perf");

export interface PerfMetric {
  name: string;
  unit: "ms" | "count";
  n?: number;
  p50?: number;
  p95?: number;
  max?: number;
  value?: number;
  budget?: number;
  mode?: string;
}

export interface PerfSampleMetric {
  name: string;
  unit: PerfMetric["unit"];
  samples: number[];
  budget?: number;
  mode?: string;
}

export interface PerfSummary {
  n: number;
  p50: number;
  p95: number;
  max: number;
}

export function summarizePerfSamples(samples: number[]): PerfSummary {
  if (samples.length === 0) throw new Error("perf metrics require at least one sample");
  if (samples.some((sample) => !Number.isFinite(sample))) {
    throw new Error("perf metric samples must be finite");
  }
  const sorted = [...samples].sort((a, b) => a - b);
  const percentile = (p: number) => sorted[Math.min(sorted.length - 1, Math.ceil((p / 100) * sorted.length) - 1)];
  return { n: sorted.length, p50: percentile(50), p95: percentile(95), max: sorted.at(-1)! };
}

function perfDir(): string {
  return process.env.PERF_METRICS_DIR ?? defaultPerfDir;
}

function appendSummary(metric: Omit<PerfSampleMetric, "samples">, summary: PerfSummary): void {
  const dir = perfDir();
  mkdirSync(dir, { recursive: true });
  appendFileSync(
    path.join(dir, "metrics.jsonl"),
    JSON.stringify({
      name: metric.name,
      unit: metric.unit,
      n: summary.n,
      p50: summary.p50,
      p95: summary.p95,
      max: summary.max,
      ...(metric.budget === undefined ? {} : { budget: metric.budget }),
      ...(metric.mode === undefined ? {} : { mode: metric.mode }),
    }) + "\n",
  );
}

export function recordPerfSamples(metric: PerfSampleMetric): PerfSummary {
  const summary = summarizePerfSamples(metric.samples);
  appendSummary(metric, summary);
  return summary;
}

export function recordPerfEvent(row: PerfEventRow): void {
  const dir = perfDir();
  mkdirSync(dir, { recursive: true });
  appendFileSync(path.join(dir, "metrics.jsonl"), JSON.stringify(row) + "\n");
}

export function recordPerfMetric(metric: PerfMetric): void {
  const fallback = metric.p50 ?? metric.p95 ?? metric.max;
  if (metric.value === undefined && fallback === undefined) {
    throw new Error(`perf metric ${metric.name} requires a value or summary`);
  }
  const samples = metric.value === undefined ? [fallback!] : [metric.value];
  const summary = metric.n === undefined || metric.p50 === undefined || metric.p95 === undefined || metric.max === undefined
    ? summarizePerfSamples(samples)
    : { n: metric.n, p50: metric.p50, p95: metric.p95, max: metric.max };
  appendSummary(metric, summary);
}
