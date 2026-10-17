import type { StreamInput } from "./streamModel";

export function createInputQueue(run: (input: StreamInput) => Promise<unknown>, onError: (error: unknown) => void) {
  const pending: StreamInput[] = [];
  const waiters: Array<() => void> = [];
  let busy = false;
  let accepting = true;
  let held: Extract<StreamInput, { kind: "pointer" }> | null = null;
  const drain = async () => {
    busy = true;
    while (pending.length) {
      try { await run(pending.shift()!); }
      catch (error) { if (accepting) onError(error); }
    }
    busy = false;
    for (const resolve of waiters.splice(0)) resolve();
  };
  return {
    push(input: StreamInput) {
      if (!accepting) return;
      if (input.kind === "pointer") held = input.buttons ? input : null;
      const last = pending[pending.length - 1];
      if (input.kind === "pointer" && input.phase === "move" && last?.kind === "pointer" && last.phase === "move" && last.buttons === input.buttons) {
        pending[pending.length - 1] = input;
      } else if (input.kind === "wheel" && last?.kind === "wheel") {
        pending[pending.length - 1] = { ...input, deltaX: last.deltaX + input.deltaX, deltaY: last.deltaY + input.deltaY };
      } else if (pending.length < 256 || (input.kind === "pointer" && input.phase === "up")) {
        pending.push(input);
      } else {
        onError(new Error("Browser input is busy; wait for the page to respond"));
      }
      if (!busy) void drain();
    },
    flush(): Promise<void> {
      return busy || pending.length ? new Promise((resolve) => waiters.push(resolve)) : Promise.resolve();
    },
    dispose() {
      if (held) pending.push({ ...held, phase: "up", buttons: 0 });
      held = null;
      accepting = false;
      if (!busy && pending.length) void drain();
    },
  };
}
