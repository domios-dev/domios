import {
  type ChatSendRecordDatabase,
  type ChatSendRepositoryPort,
  type ChatSendPersistenceState,
} from "./chatSendRepository";
import {
  chatSendDismissalMatchedRowKeys,
  chatSendDismissalMatchesIdentity,
  chatSendRetention,
  decodeChatSend,
  durableRecordByteLength,
  encodeChatSendDismissalMarker,
  encodeChatSend,
  encodeChatSendTombstone,
  mergeChatSendDismissalMarker,
  mergeChatSendRecord,
  orderChatSendRecords,
  sameChatSendDismissalMarker,
  sameDurableRecord,
  type ChatSendDismissalIdentity,
  type ChatSendDismissalMarker,
  type ChatSendDurableRecord,
} from "./chatSendDurableModel";
import type { DeliverySend } from "./chatDeliveryModel";

const MAX_SETTLED_RECORDS = 64;

class MemoryDismissalIndex extends Map<string, ChatSendDismissalMarker> {
  private readonly byCorrelation = new Map<string, Set<string>>();
  private readonly byMatchedRow = new Map<string, Set<string>>();
  private readonly byEcho = new Map<string, Set<string>>();

  override set(key: string, marker: ChatSendDismissalMarker): this {
    const prior = this.get(key);
    if (prior) this.removeFromIndexes(key, prior);
    super.set(key, marker);
    this.addToIndex(this.byCorrelation, marker.correlationId, key, marker.tabId);
    this.addToIndex(this.byMatchedRow, marker.matchedRowKey, key, marker.tabId);
    this.addToIndex(this.byEcho, marker.echoKey, key, marker.tabId);
    return this;
  }

  override delete(key: string): boolean {
    const prior = this.get(key);
    if (prior) this.removeFromIndexes(key, prior);
    return super.delete(key);
  }

  override clear(): void {
    super.clear();
    this.byCorrelation.clear();
    this.byMatchedRow.clear();
    this.byEcho.clear();
  }

  matching(tabId: string, identities: readonly ChatSendDismissalIdentity[]): ChatSendDismissalMarker[] {
    const keys = new Set<string>();
    for (const identity of identities) {
      const direct = key(tabId, identity.id);
      if (this.has(direct)) keys.add(direct);
      this.addIndexedKeys(keys, this.byCorrelation, tabId, identity.correlationId);
      this.addIndexedKeys(keys, this.byEcho, tabId, identity.echoKey);
      for (const matchedRowKey of chatSendDismissalMatchedRowKeys(identity)) {
        this.addIndexedKeys(keys, this.byMatchedRow, tabId, matchedRowKey);
      }
    }
    return [...keys].flatMap((candidate) => {
      const marker = this.get(candidate);
      return marker && identities.some((identity) => chatSendDismissalMatchesIdentity(marker, identity))
        ? [marker] : [];
    });
  }

  private addIndexedKeys(keys: Set<string>, index: Map<string, Set<string>>, tabId: string, value?: string): void {
    if (!value) return;
    for (const candidate of index.get(indexKey(tabId, value)) ?? []) keys.add(candidate);
  }

  private addToIndex(index: Map<string, Set<string>>, value: string | undefined, recordKey: string, tabId: string): void {
    if (!value) return;
    const indexed = index.get(indexKey(tabId, value)) ?? new Set<string>();
    indexed.add(recordKey);
    index.set(indexKey(tabId, value), indexed);
  }

  private removeFromIndexes(recordKey: string, marker: ChatSendDismissalMarker): void {
    this.deleteIndexed(this.byCorrelation, marker.correlationId, marker.tabId, recordKey);
    this.deleteIndexed(this.byMatchedRow, marker.matchedRowKey, marker.tabId, recordKey);
    this.deleteIndexed(this.byEcho, marker.echoKey, marker.tabId, recordKey);
  }

  private deleteIndexed(index: Map<string, Set<string>>, value: string | undefined, tabId: string, recordKey: string): void {
    if (!value) return;
    const indexKeyValue = indexKey(tabId, value);
    const indexed = index.get(indexKeyValue);
    indexed?.delete(recordKey);
    if (indexed?.size === 0) index.delete(indexKeyValue);
  }
}

export class MemoryChatSendDatabase implements ChatSendRecordDatabase {
  readonly records = new Map<string, ChatSendDurableRecord>();
  readonly dismissals = new MemoryDismissalIndex();
  readonly writes: Array<{ tabId: string; sendId: string; bytes: number }> = [];
  readonly dismissalWrites: Array<{ tabId: string; sendId: string }> = [];
  failRead = false;
  closed = false;

  async loadTab(tabId: string): Promise<{ records: ChatSendDurableRecord[]; rowsRead: number }> {
    if (this.failRead) throw new Error("read failed");
    const tabRecords = [...this.records.values()].filter((record) => record.tabId === tabId);
    const active = tabRecords.filter((record) => record.retention === "active" && !record.deleted);
    const settled = tabRecords.filter((record) => record.retention === "settled" && !record.deleted)
      .sort((left, right) => right.order - left.order).slice(0, MAX_SETTLED_RECORDS);
    const records = orderChatSendRecords([...active, ...settled]);
    return { records, rowsRead: records.length };
  }

  async loadDismissals(tabId: string, identities: readonly ChatSendDismissalIdentity[]): Promise<ChatSendDismissalMarker[]> {
    if (this.failRead) throw new Error("read failed");
    return this.dismissals.matching(tabId, identities);
  }

  async readDismissal(tabId: string, sendId: string): Promise<ChatSendDismissalMarker | undefined> {
    if (this.failRead) throw new Error("read failed");
    return this.dismissals.get(key(tabId, sendId));
  }

  async dismiss(tabId: string, send: DeliverySend): Promise<boolean> {
    if (this.failRead) throw new Error("read failed");
    const identity = key(tabId, send.id);
    const prior = this.records.get(identity);
    const current = prior && !prior.deleted ? decodeChatSend(prior) ?? undefined : undefined;
    const merged = { ...current, ...send, dismissed: true,
      echo: send.echo ?? current?.echo,
      matchedRow: send.matchedRow ?? current?.matchedRow ?? current?.status?.matched_row } as DeliverySend;
    const order = prior?.order ?? this.nextOrder(tabId);
    const marker = encodeChatSendDismissalMarker(tabId, merged, (prior?.revision ?? -1) + 1, order);
    const priorMarker = this.dismissals.get(identity);
    const mergedMarker = mergeChatSendDismissalMarker(priorMarker, marker);
    const markerChanged = !priorMarker || !sameChatSendDismissalMarker(priorMarker, mergedMarker);
    if (markerChanged) {
      this.dismissals.set(identity, mergedMarker);
      this.dismissalWrites.push({ tabId, sendId: send.id });
    }
    if (prior?.deleted) return markerChanged;
    const durable = chatSendRetention(merged) === "active"
      ? encodeChatSend(tabId, merged, (prior?.revision ?? -1) + 1, order)
      : encodeChatSendTombstone(tabId, send.id, (prior?.revision ?? -1) + 1, order);
    const recordChanged = !prior || !sameDurableRecord(prior, durable);
    if (recordChanged) {
      this.records.set(identity, durable);
      this.writes.push({ tabId, sendId: send.id, bytes: durableRecordByteLength(durable) });
    }
    this.compactSettledOverflow(tabId);
    return markerChanged || recordChanged;
  }

  async read(tabId: string, sendId: string): Promise<ChatSendDurableRecord | undefined> {
    if (this.failRead) throw new Error("read failed");
    return this.records.get(key(tabId, sendId));
  }

  async write(tabId: string, previous: DeliverySend | undefined, next: DeliverySend) {
    const identity = key(tabId, next.id);
    const prior = this.records.get(identity);
    if (prior?.deleted) {
      const oldMarker = this.dismissals.get(identity);
      if (!oldMarker) return { record: prior, changed: false };
      const marker = mergeChatSendDismissalMarker(oldMarker, encodeChatSendDismissalMarker(
        tabId, { ...next, dismissed: true }, prior.revision + 1, prior.order,
      ));
      if (sameChatSendDismissalMarker(oldMarker, marker)) return { record: prior, changed: false };
      this.dismissals.set(identity, marker);
      this.dismissalWrites.push({ tabId, sendId: next.id });
      return { record: prior, changed: true };
    }
    const current = prior ? decodeChatSend(prior) ?? undefined : undefined;
    const merged = mergeChatSendRecord(current, previous, next);
    const order = prior?.order ?? this.nextOrder(tabId);
    const revision = (prior?.revision ?? -1) + 1;
    let markerChanged = false;
    if (merged.dismissed) {
      const marker = encodeChatSendDismissalMarker(tabId, merged, revision, order);
      const priorMarker = this.dismissals.get(identity);
      const mergedMarker = mergeChatSendDismissalMarker(priorMarker, marker);
      markerChanged = !priorMarker || !sameChatSendDismissalMarker(priorMarker, mergedMarker);
      if (markerChanged) {
        this.dismissals.set(identity, mergedMarker);
        this.dismissalWrites.push({ tabId, sendId: next.id });
      }
    }
    const durable = merged.dismissed && chatSendRetention(merged) === "settled"
      ? encodeChatSendTombstone(tabId, next.id, revision, order)
      : encodeChatSend(tabId, merged, revision, order);
    if (prior && sameDurableRecord(prior, durable) && !markerChanged) return { record: prior, changed: false };
    if (!prior || !sameDurableRecord(prior, durable)) {
      this.records.set(identity, durable);
      this.writes.push({ tabId, sendId: next.id, bytes: durableRecordByteLength(durable) });
    }
    this.compactSettledOverflow(tabId);
    return { record: durable, changed: true };
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

  private compactSettledOverflow(tabId: string): void {
    const settled = [...this.records.values()].filter((record) => record.tabId === tabId
      && record.retention === "settled" && !record.deleted).sort((left, right) => right.order - left.order);
    for (const record of settled.slice(MAX_SETTLED_RECORDS)) {
      const send = decodeChatSend(record);
      if (send?.dismissed) {
        const marker = encodeChatSendDismissalMarker(tabId, send, record.revision + 1, record.order);
        const priorMarker = this.dismissals.get(key(tabId, record.sendId));
        const merged = mergeChatSendDismissalMarker(priorMarker, marker);
        if (!priorMarker || !sameChatSendDismissalMarker(priorMarker, merged)) {
          this.dismissals.set(key(tabId, record.sendId), merged);
          this.dismissalWrites.push({ tabId, sendId: record.sendId });
        }
      }
      const tombstone = encodeChatSendTombstone(tabId, record.sendId, record.revision + 1, record.order);
      this.records.set(key(tabId, record.sendId), tombstone);
      this.writes.push({ tabId, sendId: record.sendId, bytes: durableRecordByteLength(tombstone) });
    }
  }
}

export class MemoryChatSendRepository implements ChatSendRepositoryPort {
  readonly records = new Map<string, ChatSendDurableRecord>();
  readonly dismissals = new MemoryDismissalIndex();
  readonly writes: Array<{ tabId: string; sendId: string; bytes: number }> = [];
  readonly dismissalWrites: Array<{ tabId: string; sendId: string }> = [];
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
    const tabRecords = [...this.records.values()].filter((record) => record.tabId === tabId);
    const active = tabRecords.filter((record) => record.retention === "active" && !record.deleted);
    const settled = tabRecords.filter((record) => record.retention === "settled" && !record.deleted)
      .sort((left, right) => right.order - left.order).slice(0, MAX_SETTLED_RECORDS);
    const records = orderChatSendRecords([...active, ...settled]);
    this.persistenceState = "ready";
    return {
      records,
      state: this.persistenceState,
      rowsRead: records.length,
    };
  }

  async loadDismissals(tabId: string, identities: readonly ChatSendDismissalIdentity[]): Promise<ChatSendDismissalMarker[]> {
    return this.dismissals.matching(tabId, identities);
  }

  async persistDismissal(tabId: string, send: DeliverySend): Promise<void> {
    if (this.persistGate) await this.persistGate;
    const changed = await this.databaseDismiss(tabId, send);
    if (!changed) return;
    this.persistenceState = "ready";
    for (const listener of this.listeners) listener(tabId, send.id);
  }

  async persist(tabId: string, previous: DeliverySend | undefined, next: DeliverySend): Promise<void> {
    if (this.persistGate) await this.persistGate;
    const identity = key(tabId, next.id);
    const prior = this.records.get(identity);
    if (prior?.deleted) {
      const oldMarker = this.dismissals.get(identity);
      if (!oldMarker) return;
      const marker = mergeChatSendDismissalMarker(oldMarker, encodeChatSendDismissalMarker(
        tabId, { ...next, dismissed: true }, prior.revision + 1, prior.order,
      ));
      if (sameChatSendDismissalMarker(oldMarker, marker)) return;
      this.dismissals.set(identity, marker);
      this.dismissalWrites.push({ tabId, sendId: next.id });
      return;
    }
    const current = prior ? decodeChatSend(prior) ?? undefined : undefined;
    const merged = mergeChatSendRecord(current, previous, next);
    const order = prior?.order ?? this.nextOrder(tabId);
    const revision = (prior?.revision ?? -1) + 1;
    if (merged.dismissed) {
      const marker = encodeChatSendDismissalMarker(tabId, merged, revision, order);
      const oldMarker = this.dismissals.get(identity);
      const mergedMarker = mergeChatSendDismissalMarker(oldMarker, marker);
      if (!oldMarker || !sameChatSendDismissalMarker(oldMarker, mergedMarker)) {
        this.dismissals.set(identity, mergedMarker);
        this.dismissalWrites.push({ tabId, sendId: next.id });
      }
    }
    const record = merged.dismissed && chatSendRetention(merged) === "settled"
      ? encodeChatSendTombstone(tabId, next.id, revision, order)
      : encodeChatSend(tabId, merged, revision, order);
    const recordChanged = !prior || !sameDurableRecord(prior, record);
    if (recordChanged) {
      this.records.set(identity, record);
      this.writes.push({ tabId, sendId: next.id, bytes: durableRecordByteLength(record) });
    }
    if (!recordChanged) return;
    this.compactSettledOverflow(tabId);
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

  async readDismissal(tabId: string, sendId: string): Promise<ChatSendDismissalMarker | undefined> {
    return this.dismissals.get(key(tabId, sendId));
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

  private async databaseDismiss(tabId: string, send: DeliverySend): Promise<boolean> {
    const identity = key(tabId, send.id);
    const prior = this.records.get(identity);
    const current = prior && !prior.deleted ? decodeChatSend(prior) ?? undefined : undefined;
    const merged = { ...current, ...send, dismissed: true,
      echo: send.echo ?? current?.echo,
      matchedRow: send.matchedRow ?? current?.matchedRow ?? current?.status?.matched_row } as DeliverySend;
    const order = prior?.order ?? this.nextOrder(tabId);
    const revision = (prior?.revision ?? -1) + 1;
    const marker = encodeChatSendDismissalMarker(tabId, merged, revision, order);
    const priorMarker = this.dismissals.get(identity);
    const mergedMarker = mergeChatSendDismissalMarker(priorMarker, marker);
    const markerChanged = !priorMarker || !sameChatSendDismissalMarker(priorMarker, mergedMarker);
    if (markerChanged) {
      this.dismissals.set(identity, mergedMarker);
      this.dismissalWrites.push({ tabId, sendId: send.id });
    }
    if (prior?.deleted) return markerChanged;
    const durable = chatSendRetention(merged) === "active"
      ? encodeChatSend(tabId, merged, revision, order)
      : encodeChatSendTombstone(tabId, send.id, revision, order);
    const recordChanged = !prior || !sameDurableRecord(prior, durable);
    if (recordChanged) {
      this.records.set(identity, durable);
      this.writes.push({ tabId, sendId: send.id, bytes: durableRecordByteLength(durable) });
    }
    this.compactSettledOverflow(tabId);
    return markerChanged || recordChanged;
  }

  private compactSettledOverflow(tabId: string): void {
    const settled = [...this.records.values()].filter((record) => record.tabId === tabId
      && record.retention === "settled" && !record.deleted).sort((left, right) => right.order - left.order);
    for (const record of settled.slice(MAX_SETTLED_RECORDS)) {
      const send = decodeChatSend(record);
      if (send?.dismissed) {
        const marker = encodeChatSendDismissalMarker(tabId, send, record.revision + 1, record.order);
        const identity = key(tabId, record.sendId);
        const priorMarker = this.dismissals.get(identity);
        const merged = mergeChatSendDismissalMarker(priorMarker, marker);
        if (!priorMarker || !sameChatSendDismissalMarker(priorMarker, merged)) {
          this.dismissals.set(identity, merged);
          this.dismissalWrites.push({ tabId, sendId: record.sendId });
        }
      }
      const tombstone = encodeChatSendTombstone(tabId, record.sendId, record.revision + 1, record.order);
      this.records.set(key(tabId, record.sendId), tombstone);
      this.writes.push({ tabId, sendId: record.sendId, bytes: durableRecordByteLength(tombstone) });
    }
  }
}

function key(tabId: string, sendId: string): string {
  return JSON.stringify([tabId, sendId]);
}

function indexKey(tabId: string, value: string): string {
  return JSON.stringify([tabId, value]);
}
