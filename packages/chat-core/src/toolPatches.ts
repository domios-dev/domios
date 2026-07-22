import type { EditPair } from "./toolNormalize";

export interface ParsedPatchFile {
  path: string;
  kind: "update" | "add" | "delete";
  edits: EditPair[];
}

const FILE_HEADER = /^\*\*\* (Update|Add|Delete) File: (.+)$/;

export function patchTextFromToolInput(input: Record<string, unknown> | null, toolArgs?: string): string | null {
  const patch = input?.patch ?? input?.input;
  if (typeof patch === "string" && patch.includes("*** Begin Patch")) return patch;
  return toolArgs?.includes("*** Begin Patch") ? toolArgs : null;
}

export function parseCodexPatch(patch: string): ParsedPatchFile[] {
  const files: ParsedPatchFile[] = [];
  let current: ParsedPatchFile | null = null;
  let oldLines: string[] = [];
  let newLines: string[] = [];
  const flushHunk = () => {
    if (current && (oldLines.length || newLines.length)) {
      current.edits.push({ old: oldLines.join("\n"), new: newLines.join("\n") });
    }
    oldLines = [];
    newLines = [];
  };
  const flushFile = () => {
    flushHunk();
    if (current?.edits.length) files.push(current);
    current = null;
  };
  for (const line of patch.replace(/\r\n/g, "\n").trim().split("\n")) {
    if (line === "*** Begin Patch" || line === "*** End Patch") continue;
    const header = FILE_HEADER.exec(line);
    if (header) {
      flushFile();
      current = {
        path: header[2].trim(),
        kind: header[1].toLowerCase() === "add" ? "add" : header[1].toLowerCase() === "delete" ? "delete" : "update",
        edits: [],
      };
    } else if (current && (line === "@@" || line.startsWith("@@ "))) {
      flushHunk();
    } else if (current && line.startsWith("-")) {
      oldLines.push(line.slice(1));
    } else if (current && line.startsWith("+")) {
      newLines.push(line.slice(1));
    }
  }
  flushFile();
  return files;
}
