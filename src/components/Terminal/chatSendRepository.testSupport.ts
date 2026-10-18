import {
  type ChatSendRecordDatabase,
  type ChatSendRepositoryPort,
  type ChatSendPersistenceState,
} from "./chatSendRepository";
import {
  decodeChatSend,
  durableRecordByteLength,
  encodeChatSend,
  encodeChatSendTombstone,
  mergeChatSendRecord,
  orderChatSendRecords,
  sameDurableRecord,
  type ChatSendDurableRecord,
} from "./chatSendDurableModel";
import type { DeliverySend } from "./chatDeliveryModel";

export class MemoryChatSendDatabase implements ChatSendRecordDatabase {
  readonly records = new Map<string, ChatSendDurableRecord>();
  readonly writes: Array<{ tabId: string; sendId: string; bytes: number }> = [];
  failRead = false;
  closed = false;

  async loadTab(tabId: string): Promise<ChatSendDurableRecord[]> {
    if (this.failRead) throw new Error("read failed");
    return orderChatSendRecords([...this.records.values()].filter((record) => record.tabId === tabId));
  }

  async read(tabId: string, sendId: string): Promise<ChatSendDurableRecord | undefined> {
    if (this.failRead) throw new Error("read failed");
    return this.records.get(key(tabId, sendId));
  }

  async write(tabId: string, previous: DeliverySend | undefined, next: DeliverySend) {
    const identity = key(tabId, next.id);
    const prior = this.records.get(identity);
    if (prior?.deleted) return { record: prior, changed: false };
    const current = prior ? decodeChatSend(prior) ?? undefined : undefined;
    const merged = mergeChatSendRecord(current, previous, next);
    const record = encodeChatSend(tabId, merged, (prior?.revision ?? -1) + 1, prior?.order ?? this.nextOrder(tabId));
    if (prior && sameDurableRecord(prior, record)) return { record: prior, changed: false };
    this.records.set(identity, record);
    this.writes.push({ tabId, sendId: next.id, bytes: durableRecordByteLength(record) });
    return { record, changed: true };
  }

  async remove(tabId: string, sendId: string, _previous: DeliverySend | undefined) {
    const identity = key(tabId, sendId);
    const prior = this.records.get(identity);
    if (prior?.deleted) return { record: prior, changed: false };
    const current = prior ? decodeChatSend(prior) : null;
    if (current?.status || current?.requestUncertain) return { record: prior!, changed: false };
    const tombstone = encodeChatSendTombstone(tabId, sendId, (prior?.revision ?? -1) + 1, prior?.order ?? this.nextOrder(tabId));
    this.records.set(identity, tombstone);
    this.writes.push({ tabId, sendId, bytes: durableRecordByteLength(tombstone) });
    return { record: tombstone, changed: true };
  }

  close(): void {
    this.closed = true;
  }

  private nextOrder(tabId: string): number {
    return Math.max(-1, ...[...this.records.values()].filter((record) => record.tabId === tabId).map((record) => record.order)) + 1;
  }
}

export class MemoryChatSendRepository implements ChatSendRepositoryPort {
  readonly records = new Map<string, ChatSendDurableRecord>();
  readonly writes: Array<{ tabId: string; sendId: string; bytes: number }> = [];
  private readonly listeners = new Set<(tabId: string, sendId: string) => void>();
  private persistenceState: ChatSendPersistenceState = "ready";
  hydrationGate: Promise<void> | null = null;
  persistGate: Promise<void> | null = null;
  failHydration = false;

  async hydrateTab(tabId: string) {
    if (this.hydrationGate) await this.hydrationGate;
    if (this.failHydration) {
      this.persistenceState = "error";
      throw new Error("hydration failed");
    }
    return {
      records: orderChatSendRecords([...this.records.values()].filter((record) => record.tabId === tabId)),
      state: this.persistenceState,
    };
  }

  async persist(tabId: string, previous: DeliverySend | undefined, next: DeliverySend): Promise<void> {
    if (this.persistGate) await this.persistGate;
    const identity = key(tabId, next.id);
    const prior = this.records.get(identity);
    if (prior?.deleted) return;
    const current = prior ? decodeChatSend(prior) ?? undefined : undefined;
    const merged = mergeChatSendRecord(current, previous, next);
    const record = encodeChatSend(tabId, merged, (prior?.revision ?? -1) + 1, prior?.order ?? this.nextOrder(tabId));
    if (prior && sameDurableRecord(prior, record)) return;
    this.records.set(identity, record);
    this.writes.push({ tabId, sendId: next.id, bytes: durableRecordByteLength(record) });
  }

  async remove(tabId: string, sendId: string, _previous: DeliverySend | undefined): Promise<void> {
    const identity = key(tabId, sendId);
    const prior = this.records.get(identity);
    if (prior?.deleted) return;
    const current = prior ? decodeChatSend(prior) : null;
    if (current?.status || current?.requestUncertain) return;
    const record = encodeChatSendTombstone(tabId, sendId, (prior?.revision ?? -1) + 1, prior?.order ?? this.nextOrder(tabId));
    this.records.set(identity, record);
    this.writes.push({ tabId, sendId, bytes: durableRecordByteLength(record) });
  }

  async read(tabId: string, sendId: string): Promise<ChatSendDurableRecord | undefined> {
    return this.records.get(key(tabId, sendId));
  }

  subscribe(listener: (tabId: string, sendId: string) => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  state(): ChatSendPersistenceState {
    return this.persistenceState;
  }

  close(): void {
    this.listeners.clear();
  }

  private nextOrder(tabId: string): number {
    return Math.max(-1, ...[...this.records.values()].filter((record) => record.tabId === tabId).map((record) => record.order)) + 1;
  }
}

function key(tabId: string, sendId: string): string {
  return JSON.stringify([tabId, sendId]);
}
