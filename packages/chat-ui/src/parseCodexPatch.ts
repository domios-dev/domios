export interface EditPair {
  old: string;
  new: string;
}

export interface ParsedPatchFile {
  path: string;
  kind: "update" | "add" | "delete";
  edits: EditPair[];
}

const FILE_HEADER =
  /^\*\*\* (Update|Add|Delete) File: (.+)$/;

export function patchTextFromToolInput(input: Record<string, unknown> | null, toolArgs?: string): string | null {
  if (input) {
    const patch = input.patch ?? input.input;
    if (typeof patch === "string" && patch.includes("*** Begin Patch")) return patch;
  }
  if (toolArgs && toolArgs.includes("*** Begin Patch")) return toolArgs;
  return null;
}

/** Parse Codex `*** Begin Patch` apply_patch input into per-file edit hunks. */
export function parseCodexPatch(patch: string): ParsedPatchFile[] {
  const normalized = patch.replace(/\r\n/g, "\n").trim();
  if (!normalized.includes("*** Begin Patch")) return [];

  const files: ParsedPatchFile[] = [];
  let current: ParsedPatchFile | null = null;
  let oldLines: string[] = [];
  let newLines: string[] = [];

  const flushHunk = () => {
    if (!current) return;
    if (oldLines.length > 0 || newLines.length > 0) {
      current.edits.push({ old: oldLines.join("\n"), new: newLines.join("\n") });
      oldLines = [];
      newLines = [];
    }
  };

  const flushFile = () => {
    flushHunk();
    if (current && current.edits.length > 0) files.push(current);
    current = null;
  };

  for (const line of normalized.split("\n")) {
    if (line === "*** Begin Patch" || line === "*** End Patch") continue;

    const header = FILE_HEADER.exec(line);
    if (header) {
      flushFile();
      const kindWord = header[1].toLowerCase();
      current = {
        path: header[2].trim(),
        kind: kindWord === "add" ? "add" : kindWord === "delete" ? "delete" : "update",
        edits: [],
      };
      continue;
    }

    if (!current) continue;

    if (line === "@@" || line.startsWith("@@ ")) {
      flushHunk();
      continue;
    }

    if (line.startsWith("-")) {
      oldLines.push(line.slice(1));
    } else if (line.startsWith("+")) {
      newLines.push(line.slice(1));
    }
  }

  flushFile();
  return files;
}
