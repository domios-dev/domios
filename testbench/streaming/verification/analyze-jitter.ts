// Replay captured chat-stream-preview SSE events through the EXACT UI pipeline
// (displayablePreview + uncoveredPreviewTail + slice(-8)) and diff consecutive
// displayed states to attribute the live jitter.
import { readFileSync } from "node:fs";

import {
  canonicalBlob,
  displayablePreview,
  uncoveredPreviewTail,
} from "/Users/kandrey/Developer/codeterm-worktrees/lane-streaming-consilium/src/components/Terminal/streamPreviewMatch.ts";

function isSmoothTransition(prev: string[], next: string[]): boolean {
  if (prev.length === 0 || next.length === 0) return true;
  for (let drop = 0; drop < prev.length; drop++) {
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

const file = process.argv[2];
const tabFilter = process.argv[3];
const blob = canonicalBlob([]); // codex probe chat is pending/empty

let prev: string[] = [];
let total = 0;
let changedCount = 0;
let jankCount = 0;
const jankSamples: string[] = [];

for (const line of readFileSync(file, "utf-8").split("\n")) {
  const idx = line.indexOf("data:");
  if (idx < 0) continue;
  let msg: any;
  try {
    msg = JSON.parse(line.slice(idx + 5));
  } catch {
    continue;
  }
  const p = msg.data;
  if (!p || (tabFilter && p.tab_id !== tabFilter)) continue;
  total++;
  const displayed = uncoveredPreviewTail(
    displayablePreview(p.frozen.concat(p.live)),
    blob,
  ).slice(-8);
  const changed = displayed.join("\n") !== prev.join("\n");
  if (changed) {
    changedCount++;
    if (!isSmoothTransition(prev, displayed)) {
      jankCount++;
      if (jankSamples.length < 6) {
        const diff: string[] = [];
        const max = Math.max(prev.length, displayed.length);
        for (let i = 0; i < max; i++) {
          if (prev[i] !== displayed[i]) {
            diff.push(`  row${i}- ${JSON.stringify((prev[i] ?? "").slice(0, 110))}`);
            diff.push(`  row${i}+ ${JSON.stringify((displayed[i] ?? "").slice(0, 110))}`);
          }
        }
        jankSamples.push(`JANK @event${total} (rows ${prev.length}->${displayed.length}):\n${diff.join("\n")}`);
      }
    }
  }
  prev = displayed;
}

console.log(`events=${total} changedFrames=${changedCount} jankFrames=${jankCount}`);
for (const s of jankSamples) console.log(s + "\n");
