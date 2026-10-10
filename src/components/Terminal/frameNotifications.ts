export interface FrameScheduler {
  request: (callback: () => void) => number;
  cancel: (handle: number) => void;
}

export function frameNotifications(notify: (key: string) => void, scheduler: FrameScheduler) {
  const pending = new Set<string>();
  let handle: number | null = null;
  const flush = () => {
    handle = null;
    const keys = [...pending];
    pending.clear();
    for (const key of keys) notify(key);
  };
  const remove = (key: string) => {
    pending.delete(key);
    if (pending.size === 0 && handle !== null) {
      scheduler.cancel(handle);
      handle = null;
    }
  };
  return {
    enqueue(key: string) {
      pending.add(key);
      if (handle === null) handle = scheduler.request(flush);
    },
    immediate(key: string) {
      remove(key);
      notify(key);
    },
    remove,
    clear() {
      pending.clear();
      if (handle !== null) scheduler.cancel(handle);
      handle = null;
    },
  };
}
