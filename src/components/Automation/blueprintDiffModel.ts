import type { BlueprintClosureDiff, BlueprintSemanticDiff } from "../../lib/blueprintApi";
export interface BlueprintDiffRow { scope: string; node: string | null; kind: "added" | "removed" | "changed"; field: string }
export function blueprintDiffModel(diff: BlueprintClosureDiff): readonly BlueprintDiffRow[] {
  const rows: BlueprintDiffRow[] = [];
  function append(scope: string, semantic: BlueprintSemanticDiff) {
    for (const id of semantic.added) rows.push({ scope, node: id, kind: "added", field: "node" });
    for (const id of semantic.removed) rows.push({ scope, node: id, kind: "removed", field: "node" });
    for (const [node, fields] of Object.entries(semantic.changed)) for (const field of fields) rows.push({ scope, node, kind: "changed", field });
    for (const field of semantic.document_fields) rows.push({ scope, node: null, kind: "changed", field });
  }
  append("root", diff.root);
  for (const [scope, callee] of Object.entries(diff.callees)) {
    rows.push({ scope, node: null, kind: !callee.before ? "added" : !callee.after ? "removed" : "changed", field: `call pin: ${callee.before?.hash ?? "∅"} → ${callee.after?.hash ?? "∅"}` });
    if (callee.fields) append(scope, callee.fields);
  }
  return rows;
}
