// Offline scorer for a recorded capture: replays preview-events.ndjson through
// a given matcher module (old/new) with the sidecar's canonical messages.

import { readFileSync } from "node:fs";

function arg(name: string): string | undefined {
  const i = process.argv.indexOf(`--${name}`);
  return i >= 0 ? process.argv[i + 1] : undefined;
}

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

async function main(): Promise<void> {
  const eventsPath = arg("events");
  const chatPath = arg("chat");
  const tab = arg("tab");
  const matcherPath = arg("matcher");
  if (!eventsPath || !chatPath || !tab || !matcherPath) {
    console.error("usage: --events e.ndjson --chat c.ndjson --tab id --matcher path.ts");
    process.exit(2);
  }
  const m = await import(matcherPath);
  const { canonicalBlob, displayablePreview, uncoveredPreviewTail } = m;
  const mkStab = m.createPreviewStabilizerState ?? (() => null);
  const stabilize = m.stabilizePreview ?? ((_s: unknown, _t: number, rows: string[]) => rows);
  const capRows = m.capPreviewRows ?? ((rows: string[]) => rows.slice(-8));
  const smoothShared: ((p: readonly string[], n: readonly string[]) => boolean) | null =
    m.isSmoothPreviewTransition ?? null;

  const finalContents: string[] = [];
  for (const line of readFileSync(chatPath, "utf-8").split("\n")) {
    if (!line.trim()) continue;
    const row = JSON.parse(line);
    if (row.kind !== "messages") continue;
    for (const msg of row.messages ?? []) finalContents.push(msg.content ?? "");
  }
  const finalBlob = canonicalBlob(finalContents.slice(-20));
  const emptyBlob = canonicalBlob([]);

  interface Ev {
    turn_seq: number;
    frozen: string[];
    live: string[];
  }
  const events: Ev[] = [];
  for (const line of readFileSync(eventsPath, "utf-8").split("\n")) {
    if (!line.trim()) continue;
    const i = line.indexOf("data:");
    let msg: any;
    try {
      msg = JSON.parse(i >= 0 ? line.slice(i + 5) : line);
    } catch {
      continue;
    }
    const p = msg.data ?? msg;
    if (!p || p.tab_id !== tab || !Array.isArray(p.frozen)) continue;
    events.push(p);
  }

  for (const [mode, blob] of [
    ["empty-blob", emptyBlob],
    ["final-blob", finalBlob],
  ] as const) {
    let prev: string[] = [];
    let prevSeq = -1;
    let jank = 0;
    let changed = 0;
    let thrash = 0;
    let dupes = 0;
    let strictOnly = 0;
    let maxRows = 0;
    const leftoversByTurn = new Map<number, string[]>();
    const jankSamples: string[] = [];
    const stabState = mkStab();
    for (const ev of events) {
      if (ev.turn_seq !== prevSeq) {
        prev = [];
        prevSeq = ev.turn_seq;
      }
      const content = displayablePreview(ev.frozen.concat(ev.live));
      const d = stabilize(
        stabState,
        ev.turn_seq,
        capRows(uncoveredPreviewTail(content, blob)),
        content.length > 0,
        blob,
      );
      if (d.join("\n") !== prev.join("\n")) {
        changed++;
        if (d.length !== prev.length) thrash++;
        if (smoothShared && !smoothShared(prev, d)) strictOnly++;
        if (!(smoothShared ? smoothShared(prev, d) : isSmoothTransition(prev, d))) {
          jank++;
          if (jankSamples.length < 3)
            jankSamples.push(
              JSON.stringify({ seq: ev.turn_seq, prev: prev.slice(0, 3), next: d.slice(0, 3) }),
            );
        }
      }
      if (mode === "final-blob") {
        for (const row of d) {
          const n = row.replace(/\s+/g, " ").trim();
          if (n.length >= 16 && blob.includes(n)) dupes++;
        }
      }
      maxRows = Math.max(maxRows, d.length);
      leftoversByTurn.set(ev.turn_seq, d);
      prev = d;
    }
    const leftoverTurns = [...leftoversByTurn.entries()].filter(([, d]) => d.length > 0);
    console.log(
      JSON.stringify({
        mode,
        events: events.length,
        turns: leftoversByTurn.size,
        changed,
        jank,
        strict_raw_only_jank: strictOnly,
        height_thrash: thrash,
        max_rows: maxRows,
        dupes: mode === "final-blob" ? dupes : undefined,
        leftover_turns_final_state: mode === "final-blob" ? leftoverTurns.length : undefined,
        leftover_samples:
          mode === "final-blob"
            ? leftoverTurns.slice(0, 3).map(([seq, d]) => ({ seq, rows: d.slice(0, 4) }))
            : undefined,
      }),
    );
    for (const s of jankSamples) console.log("  JANK", s.slice(0, 400));
  }
}

void main();
