import type { ActivityMessage } from "./activity";
import type { PatchEdit } from "./patchEdits";

export interface PendingFileChange {
  turnId: string;
  fingerprint: string;
}

export function editFingerprint(edits: PatchEdit[]): string {
  if (!edits.length) return "";
  const files = new Map<string, { old: string[]; new: string[] }>();
  for (const edit of edits) {
    if (!edit.path || typeof edit.old !== "string" || typeof edit.new !== "string") return "";
    const lines = files.get(edit.path) ?? { old: [], new: [] };
    if (edit.old) lines.old.push(edit.old);
    if (edit.new) lines.new.push(edit.new);
    files.set(edit.path, lines);
  }
  return JSON.stringify([...files].sort(([a], [b]) => a < b ? -1 : a > b ? 1 : 0)
    .map(([path, lines]) => [path, lines.old.join("\n"), lines.new.join("\n")]));
}

export function pairFileChange(
  activity: ActivityMessage | null,
  turnId: unknown,
  pending: Map<string, PendingFileChange>,
): ActivityMessage | null {
  if (!activity || typeof turnId !== "string" || !turnId) return activity;
  const call = activity.blocks.find(block => block.kind === "tool_use")?.data;
  if (call?.name !== "file_change" || !Array.isArray(call.file_diffs) || !call.file_diffs.length) return activity;
  const edits = call.tool_edits as PatchEdit[];
  const fingerprint = editFingerprint(edits);
  const paths = (call.input as { paths?: string[] })?.paths;
  if (!fingerprint || !paths || paths.length !== new Set(edits.map(edit => edit.path)).size) return activity;
  const matches = [...pending].filter(([, entry]) => entry.turnId === turnId && entry.fingerprint === fingerprint);
  if (matches.length !== 1) return activity;
  const [id] = matches[0];
  pending.delete(id);
  const result = activity.blocks.find(block => block.kind === "tool_result")?.data;
  return { ...activity, blocks: [{ kind: "tool_result", data: {
    ...result, tool_use_id: id, file_diffs: call.file_diffs,
  } }] };
}
