import type { FileDiff, FileDiffHunk } from "./types";

function headerPath(value: string): string {
  const raw = value.split("\t", 1)[0];
  try { return raw.startsWith('"') ? JSON.parse(raw) as string : raw; } catch { return ""; }
}

export function parseUnifiedDiff(patch: string): FileDiff[] {
  const files: FileDiff[] = [];
  const lines = patch.replace(/\r\n/g, "\n").replace(/\n$/, "").split("\n");
  for (let i = 0; i < lines.length; i++) {
    if (!lines[i].startsWith("--- ") || !lines[i + 1]?.startsWith("+++ ")) continue;
    const before = headerPath(lines[i].slice(4));
    const after = headerPath(lines[++i].slice(4));
    if (!before || !after) continue;
    const hunks: FileDiffHunk[] = [];
    let invalid = false;
    while (i + 1 < lines.length && lines[i + 1].startsWith("@@ ")) {
      const header = /^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@/.exec(lines[++i]);
      if (!header) { invalid = true; break; }
      const hunk: FileDiffHunk = {oldStart:Number(header[1]), oldLines:Number(header[2] ?? 1), newStart:Number(header[3]), newLines:Number(header[4] ?? 1), lines:[]};
      let old = 0, next = 0;
      while (i + 1 < lines.length && (old < hunk.oldLines || next < hunk.newLines || lines[i + 1].startsWith("\\"))) {
        const line = lines[++i], marker = line[0];
        if (!marker || !" +-\\".includes(marker)) { invalid = true; break; }
        hunk.lines.push(line);
        if (marker === " " || marker === "-") old++;
        if (marker === " " || marker === "+") next++;
      }
      if (old !== hunk.oldLines || next !== hunk.newLines) invalid = true;
      hunks.push(hunk);
      if (invalid) break;
    }
    if (invalid || !hunks.length) continue;
    const content = hunks.flatMap(hunk => hunk.lines);
    const path = (before === "/dev/null" ? after : before).replace(/^[ab]\//, "");
    const destination = after.replace(/^b\//, "");
    const kind = before === "/dev/null" ? "add" : after === "/dev/null" ? "delete" : path !== destination ? "rename" : "modify";
    files.push({path, kind, ...(kind === "rename" ? {movePath:destination} : {}), hunks,
      additions:content.filter(line => line.startsWith("+")).length,
      deletions:content.filter(line => line.startsWith("-")).length});
  }
  return files;
}
