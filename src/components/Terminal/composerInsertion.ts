import type { AttachmentKind } from "@codeterm/chat-core";
import { getCurrentTabUiMode, setClientTabUiMode } from "../../lib/clientTabUiMode";

/**
 * Context a caller adds to a tab's composer as one inline capsule. `content` is stored to a temp file
 * so the agent always reads it by path; `path` points at an existing file instead.
 */
export interface ContextCapsule {
  kind: AttachmentKind;
  path?: string;
  content?: string;
  label?: string;
  /** For numbered kinds ("quote1 · <detail>"). */
  detail?: string;
  fileName?: string;
}

const pending = new Map<string, ContextCapsule[]>();
const listeners = new Set<() => void>();

export const composerInsertionStore = {
  insert(tabId: string, capsules: readonly ContextCapsule[]) {
    pending.set(tabId, [...(pending.get(tabId) ?? []), ...capsules]);
    for (const listener of listeners) listener();
  },
  take(tabId: string): ContextCapsule[] {
    const queued = pending.get(tabId) ?? [];
    pending.delete(tabId);
    return queued;
  },
  clear() { pending.clear(); },
  subscribe(listener: () => void) { listeners.add(listener); return () => { listeners.delete(listener); }; },
};

function slug(text: string): string {
  return text.toLowerCase().replace(/[^a-z0-9.]+/g, "-").replace(/^-+|-+$/g, "").slice(0, 48) || "context";
}

/** The temp file name a content capsule is stored under. */
export function contextFileName(capsule: ContextCapsule): string {
  if (capsule.fileName) return capsule.fileName;
  const hint = capsule.detail ?? capsule.label ?? capsule.kind;
  return `${capsule.kind}-${slug(hint)}.${capsule.kind === "diff" ? "diff" : "md"}`;
}

/** Shows the tab's chat and adds the capsules to its composer; the composer never sends them on its own. */
export function insertIntoComposer(tabId: string, capsules: ContextCapsule | readonly ContextCapsule[]) {
  const list = Array.isArray(capsules) ? capsules : [capsules as ContextCapsule];
  if (list.length === 0) return;
  if (getCurrentTabUiMode(tabId) !== "chat") setClientTabUiMode(tabId, "chat");
  composerInsertionStore.insert(tabId, list);
}
