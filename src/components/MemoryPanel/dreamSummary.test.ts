import type { GraphWorkerInfo } from "../../lib/types";
import { dreamSummary, summarizeDreamLog } from "./dreamVisuals";
import { dreamRunReport } from "./dreamRunStatus";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function log(lines: string[]): string {
  return lines.join("\n");
}

function dreamSummary_prefers_worker_report() {
  const report = "report-token-7f3a";
  const summary = dreamSummary({
    report: `\n  ${report}  \nsecond-line-token`,
    log: log(["DREAM_BACKLINKS: 178"]),
  });
  assert(summary?.source === "report", `report must win over the log: ${JSON.stringify(summary)}`);
  assert(summary.text === report, `summary must be the report's first line: ${summary.text}`);

  const blank = dreamSummary({ report: "   \n ", log: log(["DREAM_BACKLINKS: 178"]) });
  assert(blank?.source === "log", "a blank report must fall back to the log counters");
  console.log("✓ dreamSummary_prefers_worker_report");
}

function dreamSummary_derives_from_log_counters() {
  const summary = dreamSummary({
    report: null,
    log: log([
      "STARTED: 2026-09-11T08:30:00Z",
      "PATTERN: repeat bridging",
      "DREAM_BACKLINKS: 178",
      "PARALLEL_EDGES: collapsed=43",
      "HUBS_BEFORE: 9",
      "HUBS: hub_a (degree 23) -> decomposed into topic_x",
      "HUBS_AFTER: 4",
      "ISLANDS: bridged=0 promoted=1 deleted=0",
      "ORPHANS: bridged=2 deleted=pending",
      "DEDUP: aliases=0 duplicates=0",
      "VECTOR_HYGIENE: pruned=pending",
    ]),
  });
  assert(summary?.source === "log", `expected a log-derived summary: ${JSON.stringify(summary)}`);
  for (const value of ["178", "43", "9", "4", "1", "2"]) {
    assert(
      new RegExp(`\\b${value}\\b`).test(summary.text),
      `summary must carry supplied counter ${value}: ${summary.text}`,
    );
  }
  assert(!/\b0\b/.test(summary.text), `zero counters must drop out: ${summary.text}`);
  assert(!summary.text.includes("pending"), `unfilled counters must drop out: ${summary.text}`);
  assert(!summary.text.includes("\n"), "the summary is one line");

  const bare = summarizeDreamLog(log(["VECTOR_HYGIENE: 12"]));
  assert(bare !== null && /\b12\b/.test(bare), `a bare leading count must be read: ${bare}`);
  const unchangedHubs = summarizeDreamLog(log(["HUBS_BEFORE: 3", "HUBS_AFTER: 3"]));
  assert(unchangedHubs === null, `unchanged hub counts add nothing: ${unchangedHubs}`);
  console.log("✓ dreamSummary_derives_from_log_counters");
}

function dreamSummary_empty_when_no_data() {
  assert(dreamSummary({}) === null, "no report and no log yields nothing");
  assert(dreamSummary({ report: "", log: "" }) === null, "empty inputs yield nothing");
  const skeleton = log(["STARTED: 2026-09-11T08:30:00Z", "PATTERN: pending", "CALIBRATION: pending"]);
  assert(dreamSummary({ log: skeleton }) === null, "a skeleton log with no counters yields nothing");
  const zeros = log(["DREAM_BACKLINKS: 0", "ISLANDS: bridged=0 promoted=0 deleted=0"]);
  assert(dreamSummary({ log: zeros }) === null, "all-zero counters yield nothing");
  console.log("✓ dreamSummary_empty_when_no_data");
}

function worker(overrides: Partial<GraphWorkerInfo>): GraphWorkerInfo {
  return {
    worker_id: "w",
    tab_id: null,
    intent: "dreaming:all",
    spawned_by: "cron",
    spawned_at: 0,
    status: "done",
    progress_text: null,
    touched_nodes: [],
    color: "#fff",
    report: null,
    done_at: null,
    ...overrides,
  } as GraphWorkerInfo;
}

function dreamRunReport_matches_diary_identity() {
  const workers = [
    worker({ worker_id: "newer-empty", diary_id: "d1", report: "  " }),
    worker({ worker_id: "other", diary_id: "d2", report: "other-token" }),
    worker({ worker_id: "owner", diary_id: "d1", report: "owner-token" }),
  ];
  assert(dreamRunReport("d1", workers) === "owner-token", "the diary's own run report must be used");
  assert(dreamRunReport("d3", workers) === null, "an unknown diary has no report");
  console.log("✓ dreamRunReport_matches_diary_identity");
}

dreamSummary_prefers_worker_report();
dreamSummary_derives_from_log_counters();
dreamSummary_empty_when_no_data();
dreamRunReport_matches_diary_identity();
