export interface SseMessage {
  event: string;
  data: string;
  id?: string;
}

export interface SseParser {
  push(chunk: string): SseMessage[];
}

const DEFAULT_EVENT = "message";
const LINE_BREAK = /[\r\n]/g;

interface PendingEvent {
  event: string;
  data: string[];
  id?: string;
}

function emptyEvent(): PendingEvent {
  return { event: "", data: [] };
}

function splitField(line: string): [string, string] {
  const colon = line.indexOf(":");
  if (colon < 0) return [line, ""];
  const value = line.slice(colon + 1);
  return [line.slice(0, colon), value.startsWith(" ") ? value.slice(1) : value];
}

export function createSseParser(): SseParser {
  let buffer = "";
  let scanFrom = 0;
  let started = false;
  let skipLeadingLf = false;
  let pending = emptyEvent();

  const dispatch = (out: SseMessage[]) => {
    if (pending.data.length > 0) {
      const message: SseMessage = { event: pending.event || DEFAULT_EVENT, data: pending.data.join("\n") };
      if (pending.id !== undefined) message.id = pending.id;
      out.push(message);
    }
    pending = emptyEvent();
  };

  const onLine = (line: string, out: SseMessage[]) => {
    if (line === "") return dispatch(out);
    if (line.startsWith(":")) return;
    const [field, value] = splitField(line);
    if (field === "data") pending.data.push(value);
    else if (field === "event") pending.event = value;
    else if (field === "id" && !value.includes("\0")) pending.id = value;
  };

  return {
    push(chunk) {
      const out: SseMessage[] = [];
      let text = chunk;
      if (!started && text.length > 0) {
        started = true;
        if (text.charCodeAt(0) === 0xfeff) text = text.slice(1);
      }
      // A CRLF split across chunks must not count as two line breaks.
      if (skipLeadingLf && text.length > 0) {
        if (text.startsWith("\n")) text = text.slice(1);
        skipLeadingLf = false;
      }
      buffer += text;
      let lineStart = 0;
      LINE_BREAK.lastIndex = scanFrom;
      for (let match = LINE_BREAK.exec(buffer); match; match = LINE_BREAK.exec(buffer)) {
        const breakAt = match.index;
        onLine(buffer.slice(lineStart, breakAt), out);
        let next = breakAt + 1;
        if (buffer[breakAt] === "\r") {
          if (next < buffer.length) {
            if (buffer[next] === "\n") next += 1;
          } else {
            skipLeadingLf = true;
          }
        }
        lineStart = next;
        LINE_BREAK.lastIndex = next;
      }
      buffer = buffer.slice(lineStart);
      scanFrom = buffer.length;
      return out;
    },
  };
}
