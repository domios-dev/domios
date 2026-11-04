import { decodeCapsuleRegistry, encodeCapsuleRegistry } from "../../components/Terminal/composerCapsules";
import type { ClientCheckpoint, Draft } from "./checkpoint";
import type { CheckpointSnapshot } from "./model";

const capsuleScopePrefix = "tab-session-chat-capsules:";
const owns = (value: object, key: string) => Object.prototype.hasOwnProperty.call(value, key);
const textScope = (scope: string) => scope.startsWith(capsuleScopePrefix)
  ? `tab-session-chat:${scope.slice(capsuleScopePrefix.length)}` : scope;

export function hasComposerDraft(drafts: CheckpointSnapshot["drafts"], scope: string): boolean {
  const draft = drafts[textScope(scope)];
  return !!draft && (!scope.startsWith(capsuleScopePrefix) || owns(draft, "capsules"));
}

export function editComposerDraft(drafts: Record<string, Draft>, scope: string, value: string): void {
  const key = textScope(scope);
  const draft = drafts[key] ?? { text: "", attachmentIds: [], replyTo: null };
  if (scope.startsWith(capsuleScopePrefix)) {
    const capsules = decodeCapsuleRegistry(value);
    drafts[key] = { ...draft, capsules, attachmentIds: Object.keys(capsules) };
  } else drafts[key] = { ...draft, text: value };
}

export function readComposerDraft(drafts: CheckpointSnapshot["drafts"], scope: string): string {
  const draft = drafts[textScope(scope)];
  if (!draft) return "";
  return scope.startsWith(capsuleScopePrefix) ? encodeCapsuleRegistry(draft.capsules ?? {}) : draft.text;
}

export function restoreComposerDrafts(checkpoint: ClientCheckpoint, records: ReadonlyMap<string, string>): ClientCheckpoint {
  const restored = structuredClone(checkpoint);
  const sorted = [...records].sort(([a], [b]) => Number(a.startsWith(capsuleScopePrefix)) - Number(b.startsWith(capsuleScopePrefix)));
  for (const [scope, value] of sorted) {
    if (!hasComposerDraft(restored.drafts, scope)) editComposerDraft(restored.drafts, scope, value);
  }
  return restored;
}

export function composerDraftRecords(checkpoint: CheckpointSnapshot): Map<string, string> {
  const records = new Map<string, string>();
  for (const [scope, draft] of Object.entries(checkpoint.drafts)) {
    records.set(scope, draft.text);
    if (scope.startsWith("tab-session-chat:") && draft.capsules) {
      records.set(capsuleScopePrefix + scope.slice("tab-session-chat:".length), encodeCapsuleRegistry(draft.capsules));
    }
  }
  return records;
}
