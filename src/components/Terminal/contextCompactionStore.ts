import { frontendCacheRepository, frontendCacheScope } from "../../lib/frontendCacheRecords";
import { uid } from "../../lib/uid";
import type { NormalizedChatMessage } from "@codeterm/chat-core";
import { emptyCompactionHistory, observeCompaction, reconcileCompactionRows, type CompactionHistory, type ContextCompaction } from "./contextCompactionModel";

export interface CompactionPersistence {
  load(key: string): Promise<CompactionHistory>;
  save(key: string, history: CompactionHistory): Promise<void>;
}

function isCompaction(value: unknown): value is ContextCompaction {
  if (value == null || typeof value !== "object") return false;
  const row = value as ContextCompaction;
  return typeof row.id === "string" && (row.afterId === null || typeof row.afterId === "string")
    && Number.isFinite(row.startedAt) && (row.endedAt === undefined || Number.isFinite(row.endedAt))
    && ["running", "completed", "failed"].includes(row.state)
    && (row.providerRowId === undefined || typeof row.providerRowId === "string");
}

const persistence: CompactionPersistence = {
  async load(key) {
    const snapshot = await frontendCacheRepository.read(key);
    return {
      compacting: snapshot?.manifest.metadata.compacting === true,
      rows: (snapshot?.records ?? []).map((record) => record.value).filter(isCompaction),
    };
  },
  async save(key, history) {
    const result = await frontendCacheRepository.write(key, history.rows.map((row) => ({ kind: "compaction", id: row.id, value: row })), { compacting: history.compacting });
    if (result.refused) throw new Error("Compaction history write conflicted");
  },
};

export function compactionHistoryKey(tabId: string, sessionId: string): string {
  return `${frontendCacheScope("context-compaction") ?? "context-compaction"}:${JSON.stringify([tabId, sessionId])}`;
}

export function createCompactionStore(port: CompactionPersistence, clock = Date.now, identity: () => string = uid) {
  const histories = new Map<string, CompactionHistory>();
  const loading = new Map<string, Promise<void>>();
  const pending = new Map<string, Promise<void>>();
  const failures = new Map<string, string>();
  const listeners = new Set<() => void>();
  const emit = () => { for (const listener of listeners) listener(); };
  const ensure = (key: string): Promise<void> => {
    let request = loading.get(key);
    if (!request) {
      request = port.load(key).then((history) => { histories.set(key, history); emit(); });
      loading.set(key, request);
    }
    return request;
  };
  const update = (key: string, reduce: (history: CompactionHistory) => CompactionHistory) => {
    const next = (pending.get(key) ?? ensure(key)).then(async () => {
      if (!histories.has(key)) throw new Error("Compaction history was not loaded");
      const previous = histories.get(key) ?? emptyCompactionHistory;
      const history = reduce(previous);
      if (history === previous) return;
      histories.set(key, history);
      emit();
      await port.save(key, history);
    }).catch((error: unknown) => { failures.set(key, String(error)); emit(); });
    pending.set(key, next);
    return next;
  };
  return {
    get: (key: string) => histories.get(key) ?? emptyCompactionHistory,
    error: (key: string) => failures.get(key),
    subscribe(listener: () => void) { listeners.add(listener); return () => { listeners.delete(listener); }; },
    hydrate: (key: string) => update(key, (history) => history),
    observe(key: string, phase: Parameters<typeof observeCompaction>[1]["phase"], afterId: string | null) {
      const at = clock();
      return update(key, (history) => observeCompaction(history, { phase, at, afterId, id: identity() }));
    },
    anchor(key: string, afterId: string | null) {
      const at = clock();
      return update(key, (history) => history.compacting
        ? observeCompaction(history, { phase: "compacting", at, afterId, id: "" }) : history);
    },
    reconcile(key: string, messages: readonly NormalizedChatMessage[]) {
      const at = clock();
      return update(key, (history) => reconcileCompactionRows(history, messages, at));
    },
  };
}

export const contextCompactionStore = createCompactionStore(persistence);
