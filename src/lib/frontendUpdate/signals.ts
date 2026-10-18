import { editComposerDraft } from "./drafts";
import type { FrontendSignalChange, FrontendSignals } from "./runtime";
import type { ClientCheckpoint, Draft } from "./checkpoint";
import type { CheckpointSnapshot } from "./model";
import type { Safety } from "./model";

const capsuleScopePrefix = "tab-session-chat-capsules:";

function freezeValue<T>(value: T): T {
  if (!value || typeof value !== "object") return value;
  for (const nested of Object.values(value as Record<string, unknown>)) freezeValue(nested);
  return Object.isFrozen(value) ? value : Object.freeze(value);
}

function freezeCheckpoint(value: ClientCheckpoint): CheckpointSnapshot {
  return freezeValue(value) as CheckpointSnapshot;
}

function draftKey(scope: string): string {
  return scope.startsWith(capsuleScopePrefix)
    ? `tab-session-chat:${scope.slice(capsuleScopePrefix.length)}`
    : scope;
}

function sameCapsules(left: Draft["capsules"], right: Draft["capsules"]): boolean {
  if ((left === undefined) !== (right === undefined)) return false;
  const current = left ?? {};
  const next = right ?? {};
  const labels = Object.keys(current);
  return labels.length === Object.keys(next).length && labels.every(label => {
    const a = current[label];
    const b = next[label];
    return !!b && a.label === b.label && a.kind === b.kind && a.path === b.path && a.uploadId === b.uploadId;
  });
}

function freezeDraft(draft: Draft, previous?: CheckpointSnapshot["drafts"][string]): CheckpointSnapshot["drafts"][string] {
  const attachmentIds = previous && draft.attachmentIds === previous.attachmentIds
    ? previous.attachmentIds
    : freezeValue(draft.attachmentIds);
  const capsules = previous && draft.capsules === previous.capsules
    ? previous.capsules
    : draft.capsules ? freezeValue(draft.capsules) : undefined;
  return Object.freeze({
    ...draft,
    attachmentIds,
    ...(Object.prototype.hasOwnProperty.call(draft, "capsules") ? { capsules } : {}),
  }) as CheckpointSnapshot["drafts"][string];
}

export function createFrontendSignals(initial: ClientCheckpoint, safety: Safety, restore: (checkpoint: ClientCheckpoint) => ClientCheckpoint | void): FrontendSignals & {
  edit(checkpoint: ClientCheckpoint): void;
  editDraft(scope: string, text: string): void;
  editScroll(key: string, surface: "chat" | "terminal", value: string): void;
  signal(patch: Partial<Safety>): void;
} {
  let checkpoint = freezeCheckpoint(structuredClone(initial));
  let currentSafety = { ...safety };
  const listeners = new Set<(change: FrontendSignalChange) => void>();
  const emit = (change: FrontendSignalChange) => { for (const listener of listeners) listener(change); };
  const publishCheckpoint = (next: CheckpointSnapshot) => {
    checkpoint = next;
    emit({ type: "checkpoint", checkpoint });
  };
  return {
    checkpoint: () => structuredClone(checkpoint) as ClientCheckpoint,
    checkpointView: () => checkpoint,
    safety: () => ({ ...currentSafety }),
    hydrate(value) {
      const input = structuredClone(value as ClientCheckpoint);
      checkpoint = freezeCheckpoint(structuredClone(restore(input) ?? input));
    },
    subscribe(listener) { listeners.add(listener); return () => { listeners.delete(listener); }; },
    edit(value) { publishCheckpoint(freezeCheckpoint(structuredClone(value))); },
    editDraft(scope, text) {
      const key = draftKey(scope);
      const previous = checkpoint.drafts[key];
      if (!scope.startsWith(capsuleScopePrefix) && previous?.text === text) return;
      const draftPatch: Record<string, Draft> = previous ? { [key]: previous as unknown as Draft } : {};
      editComposerDraft(draftPatch, scope, text);
      const next = draftPatch[key];
      if (!next || (scope.startsWith(capsuleScopePrefix) && previous && sameCapsules(previous.capsules, next.capsules)
        && previous.attachmentIds.length === next.attachmentIds.length
        && previous.attachmentIds.every((id, index) => id === next.attachmentIds[index]))) return;
      const drafts = Object.freeze({ ...checkpoint.drafts, [key]: freezeDraft(next, previous) });
      publishCheckpoint(Object.freeze({ ...checkpoint, drafts }) as CheckpointSnapshot);
    },
    editScroll(key, surface, value) {
      const previous = checkpoint.scroll[key];
      if (previous?.[surface] === value) return;
      const next = { terminal: previous?.terminal ?? null, chat: previous?.chat ?? null };
      next[surface] = value;
      const scroll = Object.freeze({ ...checkpoint.scroll, [key]: Object.freeze(next) });
      publishCheckpoint(Object.freeze({ ...checkpoint, scroll }) as CheckpointSnapshot);
    },
    signal(patch) {
      currentSafety = { ...currentSafety, ...patch };
      emit({ type: "safety", safety: { ...currentSafety } });
    },
  };
}
