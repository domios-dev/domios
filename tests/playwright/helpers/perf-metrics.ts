/**
 * Perf gate metrics sink: every perf test appends one JSON line per metric to
 * artifacts/perf/metrics.jsonl (p50/p95/max or a single value), giving shard
 * machines a machine-readable trend artifact next to the hard asserts.
 */
import { appendFileSync, mkdirSync } from "node:fs";
import path from "node:path";

const PERF_DIR = path.resolve(__dirname, "..", "artifacts", "perf");

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

export function recordPerfMetric(metric: PerfMetric): void {
  try {
    mkdirSync(PERF_DIR, { recursive: true });
    const line = JSON.stringify({ ts: new Date().toISOString(), ...metric });
    appendFileSync(path.join(PERF_DIR, "metrics.jsonl"), line + "\n");
  } catch {
    /* metrics are trend data; the hard expect() asserts are the gate */
  }
}
