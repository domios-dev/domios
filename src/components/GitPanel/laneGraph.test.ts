import { assignLanes, type GraphCommitInput, type GraphRow } from "./laneGraph";

let passed = 0;
let failed = 0;

function assert(label: string, ok: boolean, detail?: string) {
  if (ok) {
    console.log(`  ✓ ${label}`);
    passed++;
  } else {
    console.error(`  ✗ ${label}${detail ? `: ${detail}` : ""}`);
    failed++;
  }
}

function byId(rows: GraphRow[]): Map<string, GraphRow> {
  return new Map(rows.map((r) => [r.sha, r]));
}

// --- linear history ---
console.log("linear history: every commit on lane 0");
{
  const commits: GraphCommitInput[] = [
    { sha: "c", parents: ["b"] },
    { sha: "b", parents: ["a"] },
    { sha: "a", parents: [] },
  ];
  const rows = assignLanes(commits);
  assert("all rows col 0", rows.every((r) => r.col === 0));
  assert("3 rows", rows.length === 3);
  assert("root has no parents", rows[2].parents.length === 0);
  assert("never widened beyond one lane", rows.every((r) => r.lanesBefore.length <= 1));
}

// --- branch + merge ---
console.log("\nbranch + merge: feature opens a lane, merge converges it");
{
  // m merges main(d) + feature(f). f branched off b.
  const commits: GraphCommitInput[] = [
    { sha: "m", parents: ["d", "f"] },
    { sha: "d", parents: ["b"] },
    { sha: "f", parents: ["b"] },
    { sha: "b", parents: ["a"] },
    { sha: "a", parents: [] },
  ];
  const rows = assignLanes(commits);
  const idx = byId(rows);
  assert("merge commit on lane 0", idx.get("m")!.col === 0);
  assert("merge has two parents", idx.get("m")!.parents.length === 2);
  assert("feature parent f got a second lane", idx.get("f")!.col === 1);
  assert("first parent d stays lane 0", idx.get("d")!.col === 0);
  // d (first parent) reserves b on lane 0; f's parent b converges into the
  // same lane, so by the time b is reached the extra lane has compacted away.
  assert("base commit b collapses to lane 0", idx.get("b")!.col === 0);
  assert("b waited for on lane 0", idx.get("b")!.lanesBefore[0] === "b");
}

// --- octopus merge (3 parents) ---
console.log("\noctopus merge: three parents do not crash, open distinct lanes");
{
  const commits: GraphCommitInput[] = [
    { sha: "o", parents: ["p1", "p2", "p3"] },
    { sha: "p1", parents: ["r"] },
    { sha: "p2", parents: ["r"] },
    { sha: "p3", parents: ["r"] },
    { sha: "r", parents: [] },
  ];
  const rows = assignLanes(commits);
  const idx = byId(rows);
  assert("octopus on lane 0", idx.get("o")!.col === 0);
  const cols = new Set([idx.get("p1")!.col, idx.get("p2")!.col, idx.get("p3")!.col]);
  assert("three parents on three distinct lanes", cols.size === 3);
  assert("root commit r exists", idx.has("r"));
}

// --- two roots ---
console.log("\ntwo roots: independent histories occupy separate lanes");
{
  const commits: GraphCommitInput[] = [
    { sha: "x2", parents: ["x1"] },
    { sha: "y2", parents: ["y1"] },
    { sha: "x1", parents: [] },
    { sha: "y1", parents: [] },
  ];
  const rows = assignLanes(commits);
  const idx = byId(rows);
  assert("first history on lane 0", idx.get("x2")!.col === 0);
  assert("second history on lane 1", idx.get("y2")!.col === 1);
  assert("roots keep their lanes", idx.get("x1")!.col === 0 && idx.get("y1")!.col === 1);
}

// --- trailing-lane compaction ---
console.log("\ntrailing-lane compaction: closed lanes shrink lanesBefore width");
{
  // feature branch closes, then a long linear tail should not keep a phantom lane.
  const commits: GraphCommitInput[] = [
    { sha: "m", parents: ["d", "f"] },
    { sha: "d", parents: ["b"] },
    { sha: "f", parents: ["b"] },
    { sha: "b", parents: ["a"] },
    { sha: "a", parents: ["z"] },
    { sha: "z", parents: [] },
  ];
  const rows = assignLanes(commits);
  const idx = byId(rows);
  // By the time we reach `a`, lane 1 (feature) is closed and trailing-compacted.
  assert("post-merge tail collapses to single lane", idx.get("a")!.lanesBefore.length <= 1);
  assert("root z single lane", idx.get("z")!.lanesBefore.length <= 1);
}

// --- determinism ---
console.log("\ndeterminism: same input yields identical output");
{
  const commits: GraphCommitInput[] = [
    { sha: "m", parents: ["d", "f"] },
    { sha: "d", parents: ["b"] },
    { sha: "f", parents: ["b"] },
    { sha: "b", parents: ["a"] },
    { sha: "a", parents: [] },
  ];
  const a = JSON.stringify(assignLanes(commits));
  const b = JSON.stringify(assignLanes(commits));
  assert("two runs identical", a === b);
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) throw new Error(`${failed} test(s) failed`);
