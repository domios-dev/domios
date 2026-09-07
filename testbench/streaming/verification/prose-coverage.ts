
import { readFileSync } from "node:fs";

function arg(name: string): string | undefined {
  const i = process.argv.indexOf(`--${name}`);
  return i >= 0 ? process.argv[i + 1] : undefined;
}

const norm = (s: string) =>
  s
    .replace(/[*_`~#|>\\]/g, "")
    .replace(/\s+/g, " ")
    .trim();

async function main(): Promise<void> {
  const eventsPath = arg("events")!;
  const chatPath = arg("chat")!;
  const tab = arg("tab")!;
  const matcherPath = arg("matcher")!;
  const m = await import(matcherPath);
  const { displayablePreview, uncoveredPreviewTail, canonicalBlob } = m;
  const mkStab = m.createPreviewStabilizerState ?? (() => null);
  const stabilize = m.stabilizePreview ?? ((_s: unknown, _t: number, rows: string[]) => rows);
  const capRows = m.capPreviewRows ?? ((rows: string[]) => rows.slice(-8));
  const stabState = mkStab();

  // Collect every row the preview EVER displays (empty blob: max retention view).
  const shown: string[] = [];
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
    const content = displayablePreview(p.frozen.concat(p.live));
    const d = stabilize(
      stabState,
      p.turn_seq,
      capRows(uncoveredPreviewTail(content, canonicalBlob([]))),
      content.length > 0,
      canonicalBlob([]),
    );
    for (const row of d) shown.push(norm(row));
  }
  const shownBlob = shown.join(" ");

  // Assistant prose segments from the sidecar.
  const segments: string[] = [];
  for (const line of readFileSync(chatPath, "utf-8").split("\n")) {
    if (!line.trim()) continue;
    const row = JSON.parse(line);
    if (row.kind !== "messages") continue;
    for (const msg of row.messages ?? []) {
      if ((msg.kind ?? msg.type) !== "assistant") continue;
      const content: string = msg.content ?? "";
      if (content.startsWith("<environment_context>")) continue;
      for (const raw of content.split(/[\n]+/)) {
        const seg = norm(raw);
        if (seg.length >= 20) segments.push(seg);
      }
    }
  }

  let covered = 0;
  const misses: string[] = [];
  for (const seg of segments) {
    // Overlap test: any 24-char window of the segment present in shown rows.
    const probe = seg.slice(0, 24);
    const probeMid = seg.slice(Math.max(0, Math.floor(seg.length / 2) - 12), Math.floor(seg.length / 2) + 12);
    if (shownBlob.includes(probe) || shownBlob.includes(probeMid)) covered++;
    else if (misses.length < 10) misses.push(seg.slice(0, 100));
  }
  console.log(
    JSON.stringify({
      tab,
      matcher: matcherPath.includes("old-") ? "old" : "current",
      prose_segments: segments.length,
      covered,
      coverage_pct: segments.length ? Math.round((covered / segments.length) * 100) : null,
    }),
  );
  for (const miss of misses) console.log("  MISS:", miss);
}

void main();
