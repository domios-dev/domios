import { laneTurnStatsLabel, summarizeTurnNumstat } from "./taskTurnStats";

const summary = summarizeTurnNumstat("2\t1\tsrc/a.rs\n-\t-\tassets/icon.png", "3\t0\tsrc/b.rs");
if (summary.files !== 3 || summary.additions !== 5 || summary.deletions !== 1) {
  throw new Error("tracked numstat rows are combined across staged and unstaged changes");
}

const label = laneTurnStatsLabel({ turn: 4, captured_at: 1, numstat: "1\t0\ta.ts", untracked_file_count: 2 });
if (label !== "Turn 4: 1 files, +1/−0, 2 untracked") {
  throw new Error("turn stats label includes tracked and untracked changes");
}
