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
  canonicalBlob,
  uncoveredPreviewTail,
} from "../../src/components/Terminal/streamPreviewMatch";

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

/** A frame transition is smooth when it only cuts rows from the top, appends
 * at the bottom, or extends the final row. Anything else is visible jank. */
function isSmoothTransition(prev: string[], next: string[]): boolean {
  if (prev.length === 0 || next.length === 0) return true;
  for (let drop = 0; drop <= prev.length; drop++) {
    const kept = prev.slice(drop);
    if (kept.length > next.length) continue;
    let matches = true;
    for (let i = 0; i < kept.length; i++) {
      const isLastKept = i === kept.length - 1;
      const same = next[i] === kept[i];
      const extended = isLastKept && next[i].startsWith(kept[i]);
      if (!same && !extended) {
        matches = false;
        break;
      }
    }
    if (matches) return true;
  }
  return false;
}

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
        current.leftover_rows = prevDisplayed.length;
        current.leftover_sample = prevDisplayed.slice(0, 6);
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
      };
      frozenBySeq.set(motion.turn_seq, []);
      prevSeq = motion.turn_seq;
      prevDisplayed = [];
    }
    const frozen = frozenBySeq.get(motion.turn_seq) ?? [];
    frozen.push(...motion.turn_frozen);
    frozenBySeq.set(motion.turn_seq, frozen);

    const view = frozen.concat(motion.turn_live);
    const displayed = uncoveredPreviewTail(view, canonicalBlob(landed));
    if (current) {
      if (displayed.length > 0 && current.first_preview_ms === null) {
        current.first_preview_ms = frame.t_ms;
      }
      current.peak_preview_rows = Math.max(current.peak_preview_rows, displayed.length);
      if (!isSmoothTransition(prevDisplayed, displayed)) current.jank_frames += 1;
    }
    prevDisplayed = displayed;
  }
  if (current) {
    current.leftover_rows = prevDisplayed.length;
    current.leftover_sample = prevDisplayed.slice(0, 6);
    turns.push(current);
  }

  const summary = {
    turns: turns.length,
    turns_with_preview: turns.filter((t) => t.first_preview_ms !== null).length,
    avg_lead_ms:
      turns
        .filter((t) => t.first_preview_ms !== null && t.first_canonical_ms !== null)
        .map((t) => (t.first_canonical_ms as number) - (t.first_preview_ms as number))
        .reduce((a, b, _, arr) => a + b / arr.length, 0) || null,
    turns_with_leftovers: turns.filter((t) => t.leftover_rows > 0).length,
    total_jank_frames: turns.reduce((a, t) => a + t.jank_frames, 0),
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
