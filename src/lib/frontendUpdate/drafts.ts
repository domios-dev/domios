import { decodeCapsuleRegistry, encodeCapsuleRegistry } from "../../components/Terminal/composerCapsules";
import { COMPOSER_DRAFT_PREFIX } from "../composerDraftGc";
import type { ClientCheckpoint, Draft } from "./checkpoint";

type DraftStorage = Pick<Storage, "length" | "key" | "getItem" | "setItem" | "removeItem">;
const capsuleScopePrefix = "tab-session-chat-capsules:";
const owns = (value: object, key: string) => Object.prototype.hasOwnProperty.call(value, key);
const textScope = (scope: string) => scope.startsWith(capsuleScopePrefix)
  ? `tab-session-chat:${scope.slice(capsuleScopePrefix.length)}` : scope;

export function hasComposerDraft(drafts: Record<string, Draft>, scope: string): boolean {
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

export function restoreComposerDrafts(checkpoint: ClientCheckpoint, storage: DraftStorage): ClientCheckpoint {
  const restored = structuredClone(checkpoint);
  try {
    const local: [string, string][] = [];
    for (let index = 0; index < storage.length; index++) {
      const key = storage.key(index);
      if (key?.startsWith(COMPOSER_DRAFT_PREFIX)) local.push([key.slice(COMPOSER_DRAFT_PREFIX.length), storage.getItem(key) ?? ""]);
    }
    local.sort(([a], [b]) => Number(a.startsWith(capsuleScopePrefix)) - Number(b.startsWith(capsuleScopePrefix)));
    for (const [scope, value] of local) {
      if (!hasComposerDraft(restored.drafts, scope)) editComposerDraft(restored.drafts, scope, value);
    }
  } catch {}
  for (const [scope, draft] of Object.entries(restored.drafts)) {
    writeLocalDraft(storage, scope, draft.text);
    if (scope.startsWith("tab-session-chat:") && draft.capsules) {
      writeLocalDraft(storage, capsuleScopePrefix + scope.slice("tab-session-chat:".length), encodeCapsuleRegistry(draft.capsules));
    }
  }
  return restored;
}

export function readComposerDraft(drafts: Record<string, Draft>, storage: Pick<Storage, "getItem">, scope: string): string {
  if (hasComposerDraft(drafts, scope)) {
    const draft = drafts[textScope(scope)];
    return scope.startsWith(capsuleScopePrefix) ? encodeCapsuleRegistry(draft.capsules ?? {}) : draft.text;
  }
  try { return storage.getItem(COMPOSER_DRAFT_PREFIX + scope) ?? ""; } catch { return ""; }
}

export function writeLocalDraft(storage: Pick<Storage, "setItem" | "removeItem">, scope: string, text: string): void {
  try {
    if (text) storage.setItem(COMPOSER_DRAFT_PREFIX + scope, text);
    else storage.removeItem(COMPOSER_DRAFT_PREFIX + scope);
  } catch {}
}
