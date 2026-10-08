import { fileDiffFromEdit, type FileDiff, type FileDiffHunk, type ToolEdit } from "@codeterm/chat-core";

function record(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : {};
}

function parsePatch(path: string, patch: string): FileDiff[] {
  const files: FileDiff[] = [];
  let file: FileDiff = { path, additions: 0, deletions: 0, hunks: [] };
  let hunk: FileDiffHunk | undefined;
  let before: string | undefined;
  const finish = () => {
    if (file.path && (file.hunks.length || file.binary || file.kind)) {
      file.truncated = file.hunks.some((item) => item.lines.filter((line) => /^[ -]/.test(line)).length !== item.oldLines
        || item.lines.filter((line) => /^[ +]/.test(line)).length !== item.newLines);
      files.push(file);
    }
    file = { path: "", additions: 0, deletions: 0, hunks: [] };
    hunk = undefined;
  };
  for (const line of patch.replace(/\r\n/g, "\n").split("\n")) {
    const header = /^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@/.exec(line);
    if (line.startsWith("diff --git ")) {
      finish();
      const paths = /^diff --git a\/(.+) b\/(.+)$/.exec(line);
      if (paths) {
        file.path = paths[1];
        if (paths[1] !== paths[2]) { file.movePath = paths[2]; file.kind = "rename"; }
      }
    } else if (line.startsWith("--- ") && (!hunk || hunk.lines.filter((item) => /^[ -]/.test(item)).length >= hunk.oldLines)) {
      if (file.hunks.length) finish();
      before = line.slice(4).replace(/^a\//, "");
    } else if (line.startsWith("+++ ") && before !== undefined) {
      const after = line.slice(4).replace(/^b\//, "");
      file.path = after === "/dev/null" ? before : after;
      file.kind = before === "/dev/null" ? "add" : after === "/dev/null" ? "delete" : before !== after ? "rename" : "modify";
      if (file.kind === "rename") { file.path = before; file.movePath = after; }
      before = undefined;
    } else if (header) {
      hunk = { oldStart: Number(header[1]), oldLines: Number(header[2] ?? 1),
        newStart: Number(header[3]), newLines: Number(header[4] ?? 1), lines: [] };
      file.hunks.push(hunk);
    } else if (/^(Binary files|GIT binary patch)/.test(line)) {
      file.binary = true;
    } else if (hunk && /^[ +\-\\]/.test(line)) {
      hunk.lines.push(line);
      if (line.startsWith("+")) file.additions++;
      if (line.startsWith("-")) file.deletions++;
    }
  }
  finish();
  if (files.length === 1 && path && files[0].kind !== "rename") files[0].path = path;
  return files;
}

export function opencodeFileDiffs(path: string, edits: ToolEdit[], metadata: unknown): FileDiff[] {
  const value = record(metadata);
  if (Array.isArray(value.files)) {
    return value.files.flatMap((entry) => {
      const item = record(entry);
      const filePath = typeof item.filePath === "string" ? item.filePath : "";
      return opencodeFileDiffs(filePath, [], { diff: item.diff }).map((diff) => ({ ...diff,
        ...(item.type === "add" || item.type === "delete" ? { kind: item.type } : {}),
        ...(typeof item.movePath === "string" ? { movePath: item.movePath, kind: "rename" as const } : {}),
      }));
    });
  }
  const patch = typeof value.diff === "string" ? value.diff : typeof value.patch === "string" ? value.patch : "";
  const diffs = patch ? parsePatch(path, patch) : [];
  return diffs.length ? diffs : edits.filter((edit) => edit.path && edit.old !== edit.new).map(fileDiffFromEdit);
}
