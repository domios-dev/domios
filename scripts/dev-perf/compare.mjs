// Prints a before/after table from two suite.mjs JSONL files: median renders and longest task per scenario.
//   node scripts/dev-perf/compare.mjs <before.jsonl> <after.jsonl>
import { readFileSync } from "node:fs";

const [beforePath, afterPath] = process.argv.slice(2);
if (!beforePath || !afterPath) throw new Error("usage: compare.mjs <before.jsonl> <after.jsonl>");

function load(path) {
  const byScenario = new Map();
  for (const line of readFileSync(path, "utf8").split("\n").filter(Boolean)) {
    const row = JSON.parse(line);
    const result = row.r ?? row;
    const runs = byScenario.get(row.scenario) ?? [];
    runs.push({ renders: result.renders, longest: result.longestTask, long: result.longTasks });
    byScenario.set(row.scenario, runs);
  }
  return byScenario;
}

const median = (values) => {
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.floor((sorted.length - 1) / 2)];
};
const summary = (runs) => runs
  ? `${String(median(runs.map((run) => run.renders))).padStart(7)} | ${String(Math.max(...runs.map((run) => run.longest))).padStart(4)}ms | ${String(Math.max(...runs.map((run) => run.long))).padStart(3)}`
  : "      - |    - |   -";

const before = load(beforePath);
const after = load(afterPath);
console.log("scenario               | renders before | longest | >50ms || renders after | longest | >50ms");
for (const scenario of new Set([...before.keys(), ...after.keys()])) {
  console.log(`${scenario.padEnd(22)} | ${summary(before.get(scenario))} || ${summary(after.get(scenario))}`);
}
