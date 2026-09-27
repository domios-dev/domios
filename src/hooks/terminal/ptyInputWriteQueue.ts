export type PtyInputWriter = (ptyId: string, data: string, raw?: boolean) => Promise<unknown>;
export type PtyInputWriteFailure = (ptyId: string, dataLength: number, error: unknown) => void;

interface PendingWrite {
  data: string;
  raw?: boolean;
}

interface PendingWrites {
  batches: PendingWrite[];
  nextBatch: number;
  drain: Promise<void>;
}

export class PtyInputWriteQueue {
  private readonly pending = new Map<string, PendingWrites>();
  private readonly failedPtys = new Set<string>();

  constructor(
    private readonly write: PtyInputWriter,
    private readonly onFailure: PtyInputWriteFailure,
  ) {}

  enqueue(ptyId: string, data: string, raw?: boolean): Promise<void> {
    if (!data) return Promise.resolve();

    const current = this.pending.get(ptyId);
    if (current) {
      const lastIndex = current.batches.length - 1;
      const last = current.batches[lastIndex];
      if (lastIndex >= current.nextBatch && last?.raw === raw) last.data += data;
      else current.batches.push({ data, raw });
      return current.drain;
    }

    const pending: PendingWrites = {
      batches: [{ data, raw }],
      nextBatch: 0,
      drain: Promise.resolve(),
    };
    this.pending.set(ptyId, pending);
    pending.drain = this.drain(ptyId, pending);
    return pending.drain;
  }

  private async drain(ptyId: string, pending: PendingWrites): Promise<void> {
    let failedDataLength = 0;
    let firstFailure: unknown;
    try {
      while (pending.nextBatch < pending.batches.length) {
        const batch = pending.batches[pending.nextBatch++];
        try {
          await this.write(ptyId, batch.data, batch.raw);
          this.failedPtys.delete(ptyId);
        } catch (error) {
          failedDataLength += batch.data.length;
          firstFailure ??= error;
        }
      }
    } finally {
      if (this.pending.get(ptyId) === pending) this.pending.delete(ptyId);
    }
    if (failedDataLength > 0 && !this.failedPtys.has(ptyId)) {
      this.failedPtys.add(ptyId);
      this.onFailure(ptyId, failedDataLength, firstFailure);
    }
  }
}
