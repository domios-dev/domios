export type PtyInputWriter = (ptyId: string, data: string) => Promise<unknown>;
export type PtyInputWriteFailure = (ptyId: string, dataLength: number, error: unknown) => void;

interface PendingWrites {
  data: string;
  drain: Promise<void>;
}

export class PtyInputWriteQueue {
  private readonly pending = new Map<string, PendingWrites>();

  constructor(
    private readonly write: PtyInputWriter,
    private readonly onFailure: PtyInputWriteFailure,
  ) {}

  enqueue(ptyId: string, data: string): Promise<void> {
    if (!data) return Promise.resolve();

    const current = this.pending.get(ptyId);
    if (current) {
      current.data += data;
      return current.drain;
    }

    const pending: PendingWrites = {
      data,
      drain: Promise.resolve(),
    };
    this.pending.set(ptyId, pending);
    pending.drain = this.drain(ptyId, pending);
    return pending.drain;
  }

  private async drain(ptyId: string, pending: PendingWrites): Promise<void> {
    try {
      while (pending.data) {
        const data = pending.data;
        pending.data = "";
        try {
          await this.write(ptyId, data);
        } catch (error) {
          this.onFailure(ptyId, data.length, error);
        }
      }
    } finally {
      if (this.pending.get(ptyId) === pending) this.pending.delete(ptyId);
    }
  }
}
