import { createDraftStorage, type DraftStorage } from "./draftStorage";

interface PendingDraft {
  value: string | null;
  running?: Promise<void>;
  pending: boolean;
  revision: number;
}

export class DraftPersistence {
  readonly records = new Map<string, string>();
  private readonly writes = new Map<string, PendingDraft>();
  private readonly errors = new Map<string, Error>();
  private readonly listeners = new Set<() => void>();
  private loading?: Promise<void>;
  private loaded = false;
  private pruneRevision = 0;

  constructor(private readonly storage: DraftStorage) {}

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => { this.listeners.delete(listener); };
  };

  error(scope: string): Error | undefined { return this.errors.get(scope) ?? this.errors.get(""); }

  private emit(): void { for (const listener of this.listeners) listener(); }

  initialize(): Promise<void> {
    this.loading ??= this.storage.read().then(records => {
      for (const [scope, value] of records) if (!this.records.has(scope)) this.records.set(scope, value);
      this.loaded = true;
      this.errors.delete("");
      this.emit();
    }).catch((error: unknown) => {
      this.errors.set("", error instanceof Error ? error : new Error(String(error)));
      this.emit();
    });
    return this.loading;
  }

  edit(scope: string, value: string): void {
    if (this.records.get(scope) === value && !this.errors.has(scope)) return;
    this.records.set(scope, value);
    this.enqueue(scope, value);
    this.emit();
  }

  private enqueue(scope: string, value: string | null): void {
    const write = this.writes.get(scope) ?? { value, pending: false, revision: 0 };
    write.value = value;
    write.pending = true;
    write.revision += 1;
    this.writes.set(scope, write);
    this.start(scope, write);
  }

  private start(scope: string, write: PendingDraft): void {
    if (write.running) return;
    let failedRevision: number | undefined;
    write.running = Promise.resolve().then(async () => {
      while (write.pending) {
        const value = write.value;
        const revision = write.revision;
        write.pending = false;
        try {
          await this.storage.write(scope, value);
          this.errors.delete(scope);
          this.emit();
        } catch (error) {
          this.errors.set(scope, error instanceof Error ? error : new Error(String(error)));
          write.pending = true;
          failedRevision = revision;
          this.emit();
          if (write.revision !== revision) continue;
          break;
        }
      }
    }).finally(() => {
      write.running = undefined;
      if (!write.pending) this.writes.delete(scope);
      else if (failedRevision !== write.revision) this.start(scope, write);
    });
  }

  async flush(): Promise<void> {
    await this.initialize();
    for (const [scope, write] of this.writes) this.start(scope, write);
    while ([...this.writes.values()].some(write => write.running)) {
      await Promise.all([...this.writes.values()].map(write => write.running));
    }
    if (this.errors.size) throw this.errors.values().next().value;
  }

  async prune(liveTabIds: ReadonlySet<string>): Promise<void> {
    const revision = ++this.pruneRevision;
    if (!liveTabIds.size) return;
    await this.initialize();
    if (!this.loaded || revision !== this.pruneRevision) return;
    for (const scope of this.records.keys()) {
      const match = /^(?:tab-session-chat(?:-capsules|-reply)?|chat-view):(.+)$/.exec(scope);
      if (match && !liveTabIds.has(match[1])) {
        this.records.delete(scope);
        this.enqueue(scope, null);
      }
    }
  }
}

export const composerDraftPersistence = new DraftPersistence(createDraftStorage());
