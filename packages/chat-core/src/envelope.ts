import type { MessageSegment as WireSegment } from "./gen/MessageSegment";

export type MessageSegment = WireSegment & { correlationId?: string };

export interface EnvelopeRow {
  content: string;
  segments: MessageSegment[];
  frame?: Extract<MessageSegment, { type: "frame" }>;
  correlationId?: string;
}

export function envelopeRows(segments: readonly MessageSegment[]): EnvelopeRow[] {
  const rows: EnvelopeRow[] = [];
  let pending: EnvelopeRow = { content: "", segments: [] };
  const flush = () => {
    if (pending.content.trim() || pending.segments.some((segment) => segment.type === "attachment")) {
      const previous = rows[rows.length - 1];
      if (previous?.frame && !previous.content.trim() && previous.correlationId === pending.correlationId) {
        previous.content = pending.content;
        previous.segments = pending.segments;
      } else rows.push(pending);
    }
    pending = { content: "", segments: [] };
  };
  for (const segment of segments) {
    if (segment.type === "frame") {
      flush();
      rows.push({ content: segment.body, segments: segment.segments?.length ? segment.segments : [{ type: "text", text: segment.body }], frame: segment, correlationId: segment.correlationId });
    } else {
      if (pending.segments.length && segment.correlationId !== pending.correlationId) flush();
      pending.correlationId = segment.correlationId;
      pending.segments.push(segment);
      pending.content += segment.type === "text" ? segment.text : segment.type === "paste" ? segment.body : segment.body ?? "";
    }
  }
  flush();
  return rows;
}

export function envelopeText(segments: readonly MessageSegment[]): string {
  return segments.map((segment) => segment.type === "text" ? segment.text : segment.body ?? "").join("");
}
