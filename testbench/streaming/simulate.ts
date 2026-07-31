// Offline streaming-preview UX simulation.
//
// Inputs: a motion timeline (render_timeline output with per-frame turn_frozen/
// turn_live), the chat arrival sidecar (record-chat-timeline.py NDJSON), and
// the capture epoch. Replays exactly what the Chat UI streaming block would
// display each frame — preview minus canonical coverage via the REAL matcher —
// and scores the UX: preview lead time, leftovers after canon, jank frames.
//
// Run: npx tsx testbench/streaming/simulate.ts \
//        --timeline timeline-motion.json --chat chat-timeline.ndjson \
//        --epoch-ms <captured_at_ms> --out streaming-report.json

import { readFileSync, writeFileSync } from "node:fs";

import {
  MAX_PREVIEW_ROWS,
  canonicalBlob,
  createPreviewStabilizerState,
  displayablePreview,
  isSmoothPreviewTransition,
  normalizeStreamLine,
  stabilizePreview,
  uncoveredPreviewTail,
} from "../../src/components/Terminal/streamPreviewMatch";

/** A rendered row that canon already covers is a visible duplicate. The length
 * floor keeps coincidental short fragments from counting. */
const DUPE_MIN_CHARS = 12;
function isCoveredDupe(row: string, blob: string): boolean {
  const normalized = normalizeStreamLine(row);
  return normalized.length >= DUPE_MIN_CHARS && blob.includes(normalized);
}

interface MotionJson {
  turn_seq: number;
  turn_frozen: string[];
  turn_live: string[];
}

interface FrameJson {
  t_ms: number;
  frame_index: number;
  motion: MotionJson;
}

interface ChatArrival {
  t_ms: number;
  contents: string[];
}

function arg(name: string): string | undefined {
  const index = process.argv.indexOf(`--${name}`);
  return index >= 0 ? process.argv[index + 1] : undefined;
}

function loadChatArrivals(path: string, epochMs: number): ChatArrival[] {
  const arrivals: ChatArrival[] = [];
  for (const line of readFileSync(path, "utf-8").split("\n")) {
    if (!line.trim()) continue;
    const row = JSON.parse(line);
    if (row.kind !== "messages") continue;
    arrivals.push({
      t_ms: Number(row.wall_ms) - epochMs,
      contents: row.messages.map((m: { content: string }) => m.content ?? ""),
    });
  }
  return arrivals.sort((a, b) => a.t_ms - b.t_ms);
}

// Jank definition is the REAL matcher's smooth-transition contract — the
// classifier and the display stabilizer share one implementation, no drift.
const isSmoothTransition = isSmoothPreviewTransition;

function main(): void {
  const timelinePath = arg("timeline");
  const chatPath = arg("chat");
  const epochMs = Number(arg("epoch-ms"));
  const outPath = arg("out") ?? "streaming-report.json";
  if (!timelinePath || !chatPath || !Number.isFinite(epochMs)) {
    console.error(
      "usage: tsx simulate.ts --timeline t.json --chat c.ndjson --epoch-ms N [--out report.json]",
    );
    process.exit(2);
  }

  const doc = JSON.parse(readFileSync(timelinePath, "utf-8"));
  const frames: FrameJson[] = doc.frames ?? [];
  const arrivals = loadChatArrivals(chatPath, epochMs);

  const frozenBySeq = new Map<number, string[]>();
  const stabilizer = createPreviewStabilizerState();
  let arrivalCursor = 0;
  const landed: string[] = [];
  let prevDisplayed: string[] = [];
  let prevSeq = 0;

  interface TurnStat {
    seq: number;
    started_ms: number;
    first_preview_ms: number | null;
    first_canonical_ms: number | null;
    peak_preview_rows: number;
    leftover_rows: number;
    leftover_sample: string[];
    jank_frames: number;
    /** Frames where a displayed decisive row was already covered by canon. */
    dupe_frames: number;
    /** Mid-turn frames where the displayed row count shrank (height thrash). */
    height_shrink_frames: number;
    /** Rows displayed when the next turn began, split post-hoc: covered by
     * later canon = handover (in-flight message, correct), never covered by
     * any canon = leftover (true stale junk). */
    boundary_rows: string[];
    handover_rows: number;
  }
  const turns: TurnStat[] = [];
  let current: TurnStat | null = null;

  for (const frame of frames) {
    const motion = frame.motion;
    if (!motion || motion.turn_seq === 0) continue;
    while (arrivalCursor < arrivals.length && arrivals[arrivalCursor].t_ms <= frame.t_ms) {
      landed.push(...arrivals[arrivalCursor].contents);
      if (current && current.first_canonical_ms === null) {
        current.first_canonical_ms = arrivals[arrivalCursor].t_ms;
      }
      arrivalCursor++;
    }
    if (motion.turn_seq !== prevSeq) {
      if (current) {
        current.boundary_rows = prevDisplayed;
        turns.push(current);
      }
      current = {
        seq: motion.turn_seq,
        started_ms: frame.t_ms,
        first_preview_ms: null,
        first_canonical_ms: null,
        peak_preview_rows: 0,
        leftover_rows: 0,
        leftover_sample: [],
        jank_frames: 0,
        dupe_frames: 0,
        height_shrink_frames: 0,
        boundary_rows: [],
        handover_rows: 0,
      };
      frozenBySeq.set(motion.turn_seq, []);
      prevSeq = motion.turn_seq;
      prevDisplayed = [];
    }
    const frozen = frozenBySeq.get(motion.turn_seq) ?? [];
    frozen.push(...motion.turn_frozen);
    frozenBySeq.set(motion.turn_seq, frozen);

    // Exact UI pipeline (useStreamPreview): junk filter, coverage cut, row
    // cap, smooth-or-hold stabilizer.
    const blob = canonicalBlob(landed);
    const view = displayablePreview(frozen.concat(motion.turn_live));
    const windowed = uncoveredPreviewTail(view, blob).slice(-MAX_PREVIEW_ROWS);
    const displayed = stabilizePreview(stabilizer, motion.turn_seq, windowed, view.length > 0, blob);
    if (current) {
      if (displayed.length > 0 && current.first_preview_ms === null) {
        current.first_preview_ms = frame.t_ms;
      }
      current.peak_preview_rows = Math.max(current.peak_preview_rows, displayed.length);
      if (!isSmoothTransition(prevDisplayed, displayed)) {
        current.jank_frames += 1;
        if (process.argv.includes("--dump-jank")) {
          console.error(
            `JANK turn ${current.seq} @${frame.t_ms}ms\n  prev: ${JSON.stringify(prevDisplayed)}\n  next: ${JSON.stringify(displayed)}`,
          );
        }
      }
      if (displayed.some((row) => isCoveredDupe(row, blob))) current.dupe_frames += 1;
      if (displayed.length > 0 && displayed.length < prevDisplayed.length) {
        current.height_shrink_frames += 1;
      }
    }
    prevDisplayed = displayed;
  }
  if (current) {
    current.boundary_rows = prevDisplayed;
    turns.push(current);
  }
  // Post-hoc split against the FULL conversation canon: a boundary row later
  // covered was an in-flight message (handover, correct); one never covered
  // anywhere is true stale junk (leftover).
  const fullBlob = canonicalBlob(landed);
  for (const turn of turns) {
    const stale = turn.boundary_rows.filter((row) => {
      const n = normalizeStreamLine(row);
      return n.length > 0 && !fullBlob.includes(n);
    });
    turn.handover_rows = turn.boundary_rows.length - stale.length;
    turn.leftover_rows = stale.length;
    turn.leftover_sample = stale.slice(0, 6);
    turn.boundary_rows = turn.boundary_rows.slice(0, 6);
  }

  const leads = turns
    .filter((t) => t.first_preview_ms !== null && t.first_canonical_ms !== null)
    .map((t) => (t.first_canonical_ms as number) - (t.first_preview_ms as number))
    .sort((a, b) => a - b);
  const summary = {
    turns: turns.length,
    turns_with_preview: turns.filter((t) => t.first_preview_ms !== null).length,
    avg_lead_ms: leads.length ? leads.reduce((a, b) => a + b, 0) / leads.length : null,
    min_lead_ms: leads.length ? leads[0] : null,
    median_lead_ms: leads.length ? leads[Math.floor(leads.length / 2)] : null,
    max_lead_ms: leads.length ? leads[leads.length - 1] : null,
    turns_with_leftovers: turns.filter((t) => t.leftover_rows > 0).length,
    total_jank_frames: turns.reduce((a, t) => a + t.jank_frames, 0),
    total_dupe_frames: turns.reduce((a, t) => a + t.dupe_frames, 0),
    total_height_shrink_frames: turns.reduce((a, t) => a + t.height_shrink_frames, 0),
  };

  writeFileSync(outPath, JSON.stringify({ summary, turns }, null, 2));
  console.log(JSON.stringify(summary, null, 2));
  for (const turn of turns) {
    const lead =
      turn.first_preview_ms !== null && turn.first_canonical_ms !== null
        ? `${turn.first_canonical_ms - turn.first_preview_ms}ms lead`
        : "no lead measured";
    console.log(
      `turn ${turn.seq}: preview@${turn.first_preview_ms}ms canon@${turn.first_canonical_ms}ms ` +
        `(${lead}) peak ${turn.peak_preview_rows} rows, leftovers ${turn.leftover_rows}, jank ${turn.jank_frames}`,
    );
  }
}

main();
