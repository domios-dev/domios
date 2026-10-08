import type { ChatDocument } from "./chatDocuments";

export interface DocumentFileCheck { paths: string[]; home: string | null; }
export interface DocumentSnapshot { documents: readonly ChatDocument[]; home: string | null; }
const empty: DocumentSnapshot = { documents: [], home: null };

export function existingDocuments(documents: readonly ChatDocument[], paths: readonly string[]): ChatDocument[] {
  const existing = new Set(paths);
  return documents.filter((document) => existing.has(document.path));
}

export function createChatDocumentsModel(check: (paths: string[], tabId: string) => Promise<DocumentFileCheck>) {
  const sources = new Map<string, readonly ChatDocument[]>();
  const snapshots = new Map<string, DocumentSnapshot>();
  const pending = new Map<string, Promise<void>>();
  const listeners = new Set<() => void>();
  const emit = () => { for (const listener of listeners) listener(); };
  const refresh = (tabId: string): Promise<void> => {
    const inFlight = pending.get(tabId);
    if (inFlight) return inFlight;
    const source = sources.get(tabId);
    if (!source?.length) return Promise.resolve();
    const request = check([...new Set(source.map((document) => document.path))], tabId).then((result) => {
      if (sources.get(tabId) !== source) return;
      snapshots.set(tabId, { documents: existingDocuments(source, result.paths), home: result.home });
      emit();
    }, () => {
      if (sources.get(tabId) !== source) return;
      snapshots.set(tabId, empty);
      emit();
    }).finally(() => { if (pending.get(tabId) === request) pending.delete(tabId); });
    pending.set(tabId, request);
    return request;
  };
  return {
    set(tabId: string, documents: readonly ChatDocument[]) {
      if (JSON.stringify(sources.get(tabId)) === JSON.stringify(documents)) return;
      sources.set(tabId, documents);
      pending.delete(tabId);
      snapshots.set(tabId, empty);
      emit();
      void refresh(tabId);
    },
    refresh,
    clear() { sources.clear(); snapshots.clear(); pending.clear(); emit(); },
    get(tabId: string) { return snapshots.get(tabId) ?? empty; },
    subscribe(listener: () => void) { listeners.add(listener); return () => { listeners.delete(listener); }; },
  };
}
