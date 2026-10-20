export type ChatAttachOutcome<T> = { kind: "completed"; value: T } | { kind: "superseded" };

export interface ChatAttachFlight<T> {
  run(key: string, start: () => Promise<T>): Promise<ChatAttachOutcome<T>>;
  cancelPending(): void;
}

export function createChatAttachFlight<T>(): ChatAttachFlight<T> {
  interface Request {
    key: string;
    start: () => Promise<T>;
    promise: Promise<ChatAttachOutcome<T>>;
    resolve: (outcome: ChatAttachOutcome<T>) => void;
    reject: (reason: unknown) => void;
  }
  let active: Request | null = null;
  let pending: Request | null = null;
  function cancelPending() {
    pending?.resolve({ kind: "superseded" });
    pending = null;
  }
  async function execute(request: Request): Promise<void> {
    active = request;
    try {
      request.resolve({ kind: "completed", value: await request.start() });
    } catch (error) {
      request.reject(error);
    } finally {
      active = null;
      const next = pending;
      pending = null;
      if (next) void execute(next);
    }
  }
  return {
    run(key, start) {
      if (active?.key === key) {
        cancelPending();
        return active.promise;
      }
      if (pending?.key === key) return pending.promise;
      let resolve!: Request["resolve"];
      let reject!: Request["reject"];
      const promise = new Promise<ChatAttachOutcome<T>>((yes, no) => { resolve = yes; reject = no; });
      const request = { key, start, promise, resolve, reject };
      if (active) {
        cancelPending();
        pending = request;
      } else {
        void execute(request);
      }
      return promise;
    },
    cancelPending,
  };
}
