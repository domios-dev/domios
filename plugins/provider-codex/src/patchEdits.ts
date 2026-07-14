export interface PatchEdit {
  path: string;
  old: string;
  new: string;
}

export function patchEdits(patch: string): PatchEdit[] {
  if (typeof patch !== "string" || patch.indexOf("*** Begin Patch") < 0) return [];
  try {
    const edits: PatchEdit[] = [];
    let path: string | null = null;
    let oldLines: string[] = [];
    let newLines: string[] = [];
    const flush = () => {
      if (path && (oldLines.length > 0 || newLines.length > 0)) edits.push({ path, old: oldLines.join("\n"), new: newLines.join("\n") });
      oldLines = [];
      newLines = [];
    };
    for (const line of patch.replace(/\r\n/g, "\n").split("\n")) {
      const header = /^\*\*\* (?:Update|Add|Delete) File: (.+)$/.exec(line);
      if (header) {
        flush();
        path = header[1].trim();
      } else if (line === "@@" || line.indexOf("@@ ") === 0) {
        flush();
      } else if (path && line.indexOf("-") === 0) {
        oldLines.push(line.substring(1));
      } else if (path && line.indexOf("+") === 0) {
        newLines.push(line.substring(1));
      }
    }
    flush();
    return edits;
  } catch (_) {
    return [];
  }
}
