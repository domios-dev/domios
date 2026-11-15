import type { AutomationAttachment, AutomationGraph, AutomationGraphAdapter } from "../Automation/types";

interface Read {
  attachment: AutomationAttachment;
  signal: AbortSignal;
  resolve: (graph: AutomationGraph) => void;
  reject: (reason: unknown) => void;
  detach: () => void;
}

export function createWorkspaceGraphReads(load: AutomationGraphAdapter["read"]): AutomationGraphAdapter["read"] {
  let active = 0;
  const pending = new Set<Read>();
  const drain = (): void => {
    while (active < 3 && pending.size) {
      const read = pending.values().next().value!;
      pending.delete(read);
      read.detach();
      active++;
      void (async () => {
        try {
          read.signal.throwIfAborted();
          read.resolve(await load(read.attachment, read.signal));
        } catch (error) { read.reject(error); }
        finally { active--; drain(); }
      })();
    }
  };
  return (attachment, signal) => new Promise((resolve, reject) => {
    if (signal.aborted) { reject(signal.reason); return; }
    const abort = () => {
      if (!pending.delete(read)) return;
      read.detach();
      reject(signal.reason);
    };
    const read: Read = { attachment, signal, resolve, reject, detach: () => signal.removeEventListener("abort", abort) };
    pending.add(read);
    signal.addEventListener("abort", abort, { once: true });
    drain();
  });
}
