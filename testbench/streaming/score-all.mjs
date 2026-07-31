// One-command scoring loop over every provider capture directory.
//
// For each captures/streaming/<provider>/ with a rendered motion timeline and
// chat sidecar, re-runs simulate.ts (the REAL matcher) and prints a compact
// per-provider summary matrix. Keeps `streaming-report.baseline.json` (first
// run) so later runs print before -> after deltas per metric.
//
// Run: node testbench/streaming/score-all.mjs [captures-base-dir]

import { execFileSync } from "node:child_process";
import { copyFileSync, existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, "../..");
const base = resolve(process.argv[2] ?? join(repoRoot, "captures/streaming"));

function readEpochMs(dir) {
  for (const name of ["meta.json", "capture-meta.json"]) {
    const path = join(dir, name);
    if (!existsSync(path)) continue;
    const meta = JSON.parse(readFileSync(path, "utf-8"));
    const epoch = meta.captured_at_ms ?? meta.capturedAtMs ?? meta.epoch_ms;
    if (Number.isFinite(Number(epoch))) return Number(epoch);
  }
  const txt = join(dir, "epoch-ms.txt");
  if (existsSync(txt)) {
    const epoch = Number(readFileSync(txt, "utf-8").trim());
    if (Number.isFinite(epoch)) return epoch;
  }
  return null;
}

function fmt(value) {
  if (value === null || value === undefined) return "—";
  return typeof value === "number" ? String(Math.round(value)) : String(value);
}

function delta(before, after) {
  if (before === null || before === undefined || after === null || after === undefined) return "";
  const diff = Math.round(after - before);
  return diff === 0 ? " (=)" : ` (${diff > 0 ? "+" : ""}${diff})`;
}

const rows = [];
if (!existsSync(base)) {
  console.error(`No capture directory found: ${base}`);
  console.error("Record sessions first with testbench/streaming/drive-recording.sh <TAB> <provider>.");
  process.exit(1);
}

for (const provider of readdirSync(base).sort()) {
  const dir = join(base, provider);
  if (!statSync(dir).isDirectory()) continue;
  const timeline = join(dir, "timeline-motion.json");
  const chat = join(dir, "chat-timeline.ndjson");
  if (!existsSync(timeline) || !existsSync(chat)) {
    rows.push({ provider, missing: "timeline/chat sidecar" });
    continue;
  }
  const epoch = readEpochMs(dir);
  if (epoch === null) {
    rows.push({ provider, missing: "captured_at_ms (meta.json / epoch-ms.txt)" });
    continue;
  }
  const report = join(dir, "streaming-report.json");
  execFileSync(
    "npx",
    ["tsx", join(here, "simulate.ts"), "--timeline", timeline, "--chat", chat, "--epoch-ms", String(epoch), "--out", report],
    { cwd: repoRoot, stdio: ["ignore", "ignore", "inherit"] },
  );
  const baselinePath = join(dir, "streaming-report.baseline.json");
  if (!existsSync(baselinePath)) copyFileSync(report, baselinePath);
  const summary = JSON.parse(readFileSync(report, "utf-8")).summary;
  const baseline = JSON.parse(readFileSync(baselinePath, "utf-8")).summary;
  rows.push({ provider, summary, baseline });
}

for (const row of rows) {
  if (row.missing) {
    console.log(`${row.provider}: SKIP — missing ${row.missing}`);
    continue;
  }
  const { summary: s, baseline: b } = row;
  console.log(
    `${row.provider}: turns ${fmt(s.turns)} | preview ${fmt(s.turns_with_preview)} | ` +
      `lead ${fmt(s.avg_lead_ms)}ms${delta(b.avg_lead_ms, s.avg_lead_ms)} | ` +
      `leftover-turns ${fmt(s.turns_with_leftovers)}${delta(b.turns_with_leftovers, s.turns_with_leftovers)} | ` +
      `jank ${fmt(s.total_jank_frames)}${delta(b.total_jank_frames, s.total_jank_frames)} | ` +
      `dupes ${fmt(s.total_dupe_frames)}${delta(b.total_dupe_frames, s.total_dupe_frames)} | ` +
      `height-shrinks ${fmt(s.total_height_shrink_frames)}${delta(
        b.total_height_shrink_frames,
        s.total_height_shrink_frames,
      )}`,
  );
}
