// Runs every surface scenario under the current load and appends one JSON line per run, then prints a summary.
//   node scripts/dev-perf/suite.mjs --port <cdp-port> --out <file.jsonl> --long-tab <tab-id> --long-workspace <name>
//     [--busy-workspace <name>] [--runs 2] [--only a,b]
import { execFileSync } from "node:child_process";
import { appendFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const args = process.argv.slice(2);
const option = (name, fallback) => { const index = args.indexOf(name); return index >= 0 ? args[index + 1] : fallback; };
const here = dirname(fileURLToPath(import.meta.url));
const out = option("--out");
const runs = Number(option("--runs", 2));
const busy = option("--busy-workspace", "codeterm");
const common = ["--port", option("--port", "9223"), "--json"];
if (option("--url-match")) common.push("--url-match", option("--url-match"));

const SUITE = [
  ["transcript-scroll", ["--tab", option("--long-tab"), "--workspace", option("--long-workspace"), "--older", "8"]],
  ["stream-idle", ["--workspace", busy]],
  ["sidebar-scroll", ["--workspace", busy]],
  ["sidebar-hover", ["--workspace", busy]],
  ["mesh-click", []],
  ["mesh-refresh", []],
  ["memory-list", []],
  ["memory-graph", []],
  ["plan-scroll", ["--workspace", busy]],
  ["plan-tasks", ["--workspace", busy]],
  ["analytics-open", []],
  ["notifications-open", []],
  ["settings-tabs", []],
];
const only = option("--only")?.split(",");

const rows = [];
for (const [scenario, extra] of SUITE) {
  if (only && !only.includes(scenario)) continue;
  for (let run = 0; run < runs; run++) {
    try {
      const line = execFileSync(process.execPath, [join(here, "surface.mjs"), scenario, ...extra, ...common], { encoding: "utf8", timeout: 120_000 }).trim().split("\n").at(-1);
      const result = { scenario, run, at: new Date().toISOString(), ...JSON.parse(line) };
      if (out) appendFileSync(out, JSON.stringify(result) + "\n");
      rows.push(result);
      console.log(`${scenario.padEnd(20)} #${run} renders=${String(result.renders).padStart(6)} commits=${String(result.commits).padStart(4)} longest=${String(result.longestTask).padStart(4)}ms long>50=${result.longTasks} ${JSON.stringify(result.detail)}`);
    } catch (error) {
      console.log(`${scenario.padEnd(20)} #${run} FAILED ${String(error.stderr || error.message).split("\n").find((text) => text.startsWith("Error")) ?? error.message}`);
    }
  }
}
