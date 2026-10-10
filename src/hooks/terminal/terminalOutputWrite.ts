import type { Terminal } from "@xterm/xterm";

export const SHELL_RESTORE_VIEWPORT_MARKER = "\x1b]777;codeterm;restored-shell-viewport\x07";
const marker = new TextEncoder().encode(SHELL_RESTORE_VIEWPORT_MARKER);
type Payload = string | Uint8Array;
type OutputTerminal = Pick<Terminal, "write"> & Partial<Pick<Terminal, "options">>;
type Write = { payload: Payload; done?: () => void };
type State = { pending: Uint8Array; queue: Write[]; writing: boolean };
const states = new WeakMap<OutputTerminal, State>();

function markerAt(bytes: Uint8Array, offset: number, length = marker.length): boolean {
  for (let i = 0; i < length; i++) {
    if (bytes[offset + i] !== marker[i]) return false;
  }
  return true;
}

function splitOutput(state: State, payload: Payload): Array<Uint8Array | "viewport"> {
  const incoming = typeof payload === "string" ? new TextEncoder().encode(payload) : payload;
  const bytes = new Uint8Array(state.pending.length + incoming.length);
  bytes.set(state.pending);
  bytes.set(incoming, state.pending.length);
  state.pending = new Uint8Array();
  const parts: Array<Uint8Array | "viewport"> = [];
  let start = 0;
  for (let i = 0; i <= bytes.length - marker.length; i++) {
    if (!markerAt(bytes, i)) continue;
    if (i > start) parts.push(bytes.slice(start, i));
    parts.push("viewport");
    i += marker.length - 1;
    start = i + 1;
  }
  let held = 0;
  for (let length = Math.min(marker.length - 1, bytes.length - start); length > 0; length--) {
    if (markerAt(bytes, bytes.length - length, length)) {
      held = length;
      break;
    }
  }
  if (bytes.length - held > start) parts.push(bytes.slice(start, bytes.length - held));
  state.pending = bytes.slice(bytes.length - held);
  return parts;
}

function clearRestoredViewport(term: OutputTerminal, done: () => void): void {
  if (!term.options) throw new Error("Shell restore requires terminal options");
  const options = term.options;
  const previous = options.scrollOnEraseInDisplay;
  // Only the shell restore boundary may turn viewport erasure into retained history.
  options.scrollOnEraseInDisplay = true;
  term.write("\x1b[?1049l\x1b[?6l\x1b[r\x1b[0m\x1b[2J\x1b[H", () => {
    options.scrollOnEraseInDisplay = previous;
    done();
  });
}

export function writeTerminalOutput(term: OutputTerminal, payload: Payload, done?: () => void): void {
  let state = states.get(term);
  if (!state) {
    state = { pending: new Uint8Array(), queue: [], writing: false };
    states.set(term, state);
  }
  state.queue.push({ payload, done });
  if (state.writing) return;
  state.writing = true;
  const drain = () => {
    const write = state.queue.shift();
    if (!write) {
      state.writing = false;
      return;
    }
    const parts = splitOutput(state, write.payload);
    let index = 0;
    const next = () => {
      const part = parts[index++];
      if (part === "viewport") clearRestoredViewport(term, next);
      else if (part) term.write(part, next);
      else {
        write.done?.();
        drain();
      }
    };
    if (parts.length === 0) term.write("", next);
    else next();
  };
  drain();
}
