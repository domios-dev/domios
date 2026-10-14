import type { ChatMsg } from "@codeterm/plugin-sdk";

export const HISTORY_CHUNK_BYTES = 512 * 1024;
const ITEM_BYTES = 64 * 1024;
const STRING_BYTES = 16 * 1024;

export function utf8Bytes(value: string): number {
  let bytes = 0;
  for (const char of value) {
    const point = char.codePointAt(0)!;
    bytes += point <= 0x7f ? 1 : point <= 0x7ff ? 2 : point <= 0xffff ? 3 : 4;
  }
  return bytes;
}
export function serializedBytes(value: unknown): number {
  return utf8Bytes(JSON.stringify(value));
}
function boundedString(value: string): string {
  const scrubbed = value.replace(/data:image\/[^\s"'<>]*;base64,[A-Za-z0-9+/=_-]*/gi,
    image => `[image omitted: ${utf8Bytes(image)} bytes]`);
  const bytes = utf8Bytes(scrubbed);
  if (bytes <= STRING_BYTES) return scrubbed;
  return `${scrubbed.slice(0, 2048)}\n[content truncated: ${bytes} bytes total]`;
}
export function sanitizeHistory(value: unknown): unknown {
  return JSON.parse(JSON.stringify(value, (_key, entry: unknown) => {
    if (typeof entry === "string") return boundedString(entry);
    if (entry && typeof entry === "object" && !Array.isArray(entry)) {
      const item = entry as Record<string, unknown>;
      if (item.type === "image" && typeof item.data === "string" && !item.data.startsWith("[image omitted:")) {
        const {data, ...rest} = item;
        return {...rest, data:`[image omitted: ${utf8Bytes(data)} bytes]`};
      }
      if (item.type === "imageGeneration" && typeof item.result === "string" && !item.result.startsWith("[image omitted:")) {
        return {...item, result:`[image omitted: ${utf8Bytes(item.result)} bytes]`};
      }
    }
    return entry;
  }));
}
export function historyItem(value: unknown): unknown {
  const sanitized = sanitizeHistory(value) as Record<string,unknown>;
  const bytes = serializedBytes(sanitized);
  return bytes <= ITEM_BYTES ? sanitized : {id:sanitized.id,type:"error",message:`[history item omitted: ${bytes} serialized bytes]`};
}
export function boundHistoryRows(rows: ChatMsg[]): ChatMsg[] {
  const bounded = rows.map(row => {
    const sanitized = sanitizeHistory(row) as ChatMsg;
    const bytes = serializedBytes(sanitized);
    if (sanitized.attachments) {
      sanitized.attachments = sanitized.attachments.map(attachment => {
        if (!attachment.url?.startsWith("[image omitted:")) return attachment;
        sanitized.content += `\n${attachment.url}`;
        const {url, ...chip} = attachment;
        return chip;
      });
    }
    if (bytes <= ITEM_BYTES) return sanitized;
    return {id:row.id,seq:row.seq,type:"system",content:`[history item omitted: ${bytes} serialized bytes]`};
  });
  const bytes = serializedBytes(bounded);
  return bytes <= ITEM_BYTES || !rows.length ? bounded : [{id:rows[0].id,seq:rows[0].seq,type:"system",content:`[history item omitted: ${bytes} serialized bytes]`}];
}
