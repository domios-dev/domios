import { writeTerminalHistory } from "./terminalHistoryWrite";

function assert(condition: boolean, message: string) {
  if (!condition) throw new Error(`FAILED: ${message}`);
}

(() => {
  const writes: Array<Uint8Array | string> = [];
  let complete = false;
  const callbacks: Array<() => void> = [];
  const term = {
    write(data: Uint8Array | string, callback?: () => void) {
      writes.push(data);
      if (callback) callbacks.push(callback);
    },
  };
  writeTerminalHistory(term, "x".repeat(100_000), () => {
    complete = true;
  });
  assert(writes.length === 1 && writes[0].length === 100_000, "history is a single xterm write");
  callbacks.shift()?.();
  assert(writes.length === 2 && writes[1] === "", "completion waits behind the xterm queue fence");
  assert(!complete, "history completion waits for the queue fence");
  callbacks.shift()?.();
  assert(complete, "history completion fires after the queue fence drains");
})();

(() => {
  let complete = false;
  const callbacks: Array<() => void> = [];
  const term = {
    write(_data: Uint8Array | string, callback?: () => void) {
      if (callback) callbacks.push(callback);
    },
  };
  const cancel = writeTerminalHistory(term, new Uint8Array(8), () => {
    complete = true;
  });
  cancel();
  callbacks.shift()?.();
  assert(!complete, "cancelled history never completes");
})();

console.log("terminal history write checks passed");
