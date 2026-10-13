import { recordedPatchDiff, type RecordedFileDiff } from "@codeterm/plugin-sdk";
import { list, object, text } from "./runtimeProtocol";

export function runtimeFileDiffs(changes: unknown): (RecordedFileDiff & {movePath?:string})[] {
  return list(changes).map(object).flatMap(change => {
    const path = text(change.path); const kind = object(change.kind); const diff = text(change.diff);
    if (!path || diff === undefined) return [];
    const selected = kind.type === "update" ? "modify" : kind.type === "add" ? "add" : kind.type === "delete" ? "delete" : undefined;
    if (!selected) return [];
    let rows:RecordedFileDiff[];
    if (/^Binary files|^GIT binary patch/m.test(diff)) rows=[{path,kind:selected,binary:true,additions:0,deletions:0,hunks:[]}];
    else if (selected === "add" || selected === "delete") {
      const lines=diff.replace(/\n$/,"").split("\n");const add=selected === "add";
      rows=[{path,kind:selected,additions:add ? lines.length : 0,deletions:add ? 0 : lines.length,
        hunks:[{oldStart:add ? 0 : 1,oldLines:add ? 0 : lines.length,newStart:add ? 1 : 0,newLines:add ? lines.length : 0,lines:lines.map(line=>(add ? "+" : "-")+line)}]}];
    } else rows=recordedPatchDiff(path,diff,"modify");
    const movePath=text(kind.move_path);
    return rows.map(row=>({...row,...(movePath ? {kind:"rename" as const,movePath} : {})}));
  });
}
