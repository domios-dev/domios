/** Bench entry point. Receives an already dev-resolved origin/token from run.sh
 *  (which sourced evals/_lib/lib.sh), runs N iterations, appends the run rows. */
import { appendFileSync, existsSync, mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { runScenario, type RunRow } from "./scenario";

const COLUMNS: Array<keyof RunRow> = [
  "origin", "tabs", "concurrency", "switches", "switch_rate_hz", "overflow_fired", "overflow_reason", "origin_switches", "overflow_per_origin_switch",
  "elapsed_ms", "resync_fanout", "mux_refetches", "refetch_errors", "output_tabs", "output_missing", "output_truncations", "max_in_flight", "refetch_max_ms", "client_gen", "daemon_gen",
  "diverged", "divergence_kind",
];

function env(name: string): string {
  const value = process.env[name];
  if (!value) throw new Error(`missing ${name} - run through evals/web-mirror-switch-sync/run.sh`);
  return value;
}

void (async () => {
  const origin = env("WMS_ORIGIN");
  const token = env("WMS_TOKEN");
  const workspaceId = env("WMS_WORKSPACE");
  const tabIds = env("WMS_TABS").split(",").filter(Boolean);
  const switches = Number(process.env.WMS_SWITCHES ?? "200");
  const concurrency = Number(process.env.WMS_CONCURRENCY ?? "1");
  const runs = Number(process.env.WMS_RUNS ?? "5");
  const skipOverflow = process.env.WMS_SKIP_OVERFLOW === "1";
  const originSwitches = Number(process.env.WMS_ORIGIN_SWITCHES ?? "0");
  // tabId=marker pairs, comma separated; each tab printed its marker from
  // its own startup command, so no PTY input is written by this bench.
  const markerSpec = process.env.WMS_MARKERS ?? "";
  const outputMarkers = markerSpec
    ? new Map(markerSpec.split(",").filter(Boolean).map((pair) => {
        const at = pair.indexOf("=");
        return [pair.slice(0, at), pair.slice(at + 1)] as [string, string];
      }))
    : null;
  const peerId = process.env.WMS_PEER_ID ?? "";
  const relayPeer = peerId ? { id: peerId, name: process.env.WMS_PEER_NAME ?? peerId } : null;

  const outDir = path.resolve(process.cwd(), "target-check/evals/web-mirror-switch-sync");
  mkdirSync(outDir, { recursive: true });
  const tsv = path.join(outDir, "runs.tsv");
  const lostOutputPath = path.join(outDir, "output-missing.txt");
  if (!existsSync(tsv)) writeFileSync(tsv, `${COLUMNS.join("\t")}\n`);

  let failures = 0;
  for (let i = 0; i < runs; i++) {
    const row = await runScenario({ origin, token, workspaceId, tabIds, switches, concurrency, skipOverflow, originSwitches,
      outputMarkers, lostOutputPath, relayPeer });
    appendFileSync(tsv, `${COLUMNS.map((c) => String(row[c])).join("\t")}\n`);
    if (row.diverged) failures++;
    console.log(
      `run ${i + 1}/${runs} origin=${row.origin} tabs=${row.tabs} conc=${row.concurrency} switches=${row.switches} ` +
      `rate=${row.switch_rate_hz}Hz overflow=${row.overflow_fired}(${row.overflow_reason}) ` +
      `fanout=${row.resync_fanout} refetch=${row.mux_refetches} err=${row.refetch_errors} inflight=${row.max_in_flight} out=${row.output_tabs - row.output_missing}/${row.output_tabs} slowest=${row.refetch_max_ms}ms ` +
      `gen=${row.client_gen}/${row.daemon_gen} ` +
      `${row.diverged ? `DIVERGED ${row.divergence_kind}` : "converged"}`,
    );
  }
  console.log(`\n${failures}/${runs} runs diverged - rows appended to ${tsv}`);
  process.exit(failures > 0 ? 1 : 0);
})();
