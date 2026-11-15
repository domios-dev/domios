export const MAX_SSE_FRAME_BYTES = 2 * 1024 * 1024;
const MAX_JSON_DEPTH = 32;
const MAX_JSON_STRUCTURAL_TOKENS = 256 * 1024;

export interface SseMessage { event: string; data: string; id?: string }
export interface SseWork {
  delimiterBytes: number;
  decodedBytes: number;
  encodedBytes: number;
  structureCodeUnits: number;
  structurePasses: number;
}

export class SseCapacityError extends Error {
  constructor(public bytes: number) { super(`SSE capacity exceeded (${bytes} bytes)`); }
}

function guardJsonStructure(value: string, bytes: number, work?: SseWork): void {
  let depth = 0, tokens = 0;
  const structural = /["{}\[\],:]/g;
  if (work) work.structurePasses++;
  while (true) {
    const start = structural.lastIndex;
    const match = structural.exec(value);
    if (work) work.structureCodeUnits += (match ? match.index + 1 : value.length) - start;
    if (!match) break;
    const character = match[0];
    if (character === '"') {
      let from = structural.lastIndex;
      while (true) {
        const quote = value.indexOf('"', from);
        if (work) work.structureCodeUnits += (quote < 0 ? value.length : quote + 1) - from;
        if (quote < 0) return;
        let slash = quote - 1;
        while (slash >= 0 && value.charCodeAt(slash) === 92) {
          if (work) work.structureCodeUnits++;
          slash--;
        }
        from = quote + 1;
        if ((quote - slash) % 2 === 1) break;
      }
      structural.lastIndex = from;
      continue;
    }
    tokens++;
    if (character === "{" || character === "[") depth++;
    else if (character === "}" || character === "]") depth--;
    if (depth > MAX_JSON_DEPTH || tokens > MAX_JSON_STRUCTURAL_TOKENS) throw new SseCapacityError(bytes);
  }
}

export class SseAccumulator {
  private buffer = new Uint8Array(4096);
  private bytes = 0;
  private decoder = new TextDecoder();

  constructor(private work?: SseWork) {}

  private append(chunk: Uint8Array): void {
    const length = this.bytes + chunk.length;
    if (length > MAX_SSE_FRAME_BYTES + 4) throw new SseCapacityError(MAX_SSE_FRAME_BYTES + 1);
    if (length > this.buffer.length) {
      const grown = new Uint8Array(Math.min(Math.max(length, this.buffer.length * 2), MAX_SSE_FRAME_BYTES + 4));
      grown.set(this.buffer.subarray(0, this.bytes));
      this.buffer = grown;
    }
    this.buffer.set(chunk, this.bytes);
    this.bytes = length;
  }

  async push(chunk: Uint8Array, deliver: (frame: string, bytes: number) => void | Promise<void>): Promise<void> {
    let start = 0;
    while (start < chunk.length) {
      const newline = chunk.indexOf(10, start);
      const end = newline < 0 ? chunk.length : newline + 1;
      if (this.work) this.work.delimiterBytes += end - start;
      this.append(chunk.subarray(start, end));
      start = end;
      const n = this.bytes;
      const b = this.buffer;
      const separator = n >= 4 && b[n - 4] === 13 && b[n - 3] === 10 && b[n - 2] === 13 && b[n - 1] === 10 ? 4
        : n >= 3 && ((b[n - 3] === 10 && b[n - 2] === 13 && b[n - 1] === 10)
          || (b[n - 3] === 13 && b[n - 2] === 10 && b[n - 1] === 10)) ? 3
        : n >= 2 && b[n - 2] === 10 && b[n - 1] === 10 ? 2 : 0;
      if (separator) {
        const bytes = n - separator;
        if (bytes > MAX_SSE_FRAME_BYTES) throw new SseCapacityError(bytes);
        const frame = this.decoder.decode(b.subarray(0, bytes));
        if (this.work) this.work.decodedBytes += bytes;
        this.bytes = 0;
        await deliver(frame, bytes);
      } else {
        const suffix = n >= 3 && b[n - 3] === 13 && b[n - 2] === 10 && b[n - 1] === 13 ? 3
          : n >= 2 && ((b[n - 2] === 13 && b[n - 1] === 10) || (b[n - 2] === 10 && b[n - 1] === 13)) ? 2
          : b[n - 1] === 13 || b[n - 1] === 10 ? 1 : 0;
        if (n - suffix > MAX_SSE_FRAME_BYTES) throw new SseCapacityError(MAX_SSE_FRAME_BYTES + 1);
      }
    }
  }

  finish(): void {
    if (this.bytes > MAX_SSE_FRAME_BYTES) throw new SseCapacityError(this.bytes);
  }
}

export function parseSseFrame(frame: string, wireBytes?: number, work?: SseWork): SseMessage | null {
  const bytes = wireBytes ?? new TextEncoder().encode(frame).length;
  if (work && wireBytes === undefined) work.encodedBytes += bytes;
  if (bytes > MAX_SSE_FRAME_BYTES) throw new SseCapacityError(bytes);
  const data: string[] = [];
  let event = "message";
  let id: string | undefined;
  for (const line of frame.split(/\r?\n/)) {
    if (line.startsWith(":")) continue;
    const colon = line.indexOf(":");
    const field = colon < 0 ? line : line.slice(0, colon);
    const value = colon < 0 ? "" : line.slice(colon + 1).replace(/^ /, "");
    if (field === "data") data.push(value);
    if (field === "event") event = value;
    if (field === "id" && !value.includes("\0")) id = value;
  }
  if (!data.length) return null;
  const payload = data.join("\n");
  guardJsonStructure(payload, bytes, work);
  return { event, data: payload, ...(id === undefined ? {} : { id }) };
}
