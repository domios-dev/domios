import { getCurrentTabUiMode, setClientTabUiMode } from "../../lib/clientTabUiMode";

/** Context for a tab's composer: file chips through its attachments, text appended to its draft. */
export interface ComposerInsertion {
  attachments?: readonly string[];
  text?: string;
}

const pending = new Map<string, ComposerInsertion[]>();
const listeners = new Set<() => void>();

export const composerInsertionStore = {
  insert(tabId: string, insertion: ComposerInsertion) {
    pending.set(tabId, [...(pending.get(tabId) ?? []), insertion]);
    for (const listener of listeners) listener();
  },
  take(tabId: string): ComposerInsertion[] {
    const queued = pending.get(tabId) ?? [];
    pending.delete(tabId);
    return queued;
  },
  clear() { pending.clear(); },
  subscribe(listener: () => void) { listeners.add(listener); return () => { listeners.delete(listener); }; },
};

/** Appends a block after a blank line and leaves the caret on a fresh line below it. */
export function appendComposerBlock(draft: string, block: string): string {
  const body = block.replace(/\s+$/, "");
  if (!body) return draft;
  const head = draft.replace(/\s+$/, "");
  return head ? `${head}\n\n${body}\n` : `${body}\n`;
}

export function applyComposerInsertions(draft: string, insertions: readonly ComposerInsertion[]) {
  return {
    draft: insertions.reduce((text, insertion) => insertion.text ? appendComposerBlock(text, insertion.text) : text, draft),
    attachments: [...new Set(insertions.flatMap((insertion) => insertion.attachments ?? []))],
  };
}

/** Shows the tab's chat and hands the composer context to add; the composer never sends it on its own. */
export function insertIntoComposer(tabId: string, insertion: ComposerInsertion) {
  if (getCurrentTabUiMode(tabId) !== "chat") setClientTabUiMode(tabId, "chat");
  composerInsertionStore.insert(tabId, insertion);
}
