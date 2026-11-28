import { buildDomiosMessage, parseDomiosMessage } from "./domios";
import type { MessageSegment } from "./envelope";
import type { NormalizedChatMessage } from "./types";
import { userEchoBody } from "./userEchoBody";

type Frame = Extract<MessageSegment, { type: "frame" }>;

export function canRetainRoutedPresentation(row: NormalizedChatMessage): boolean {
  return row.type === "user"
    && !row.segments?.some(segment => segment.type !== "text" && segment.type !== "paste" && segment.type !== "partial_frame");
}

function matchesFrameCopy(content: string, priorContent: string, frame: Frame): boolean {
  const echo = userEchoBody(content);
  if (!echo) return false;
  const prior = userEchoBody(priorContent);
  if (echo === prior || echo === userEchoBody(frame.body)) return true;
  if (!echo.startsWith("<domios ")) return false;

  const parsed = parseDomiosMessage(echo);
  if (!parsed) {
    const suffix = frame.tip == null ? "" : ` <tip>${frame.tip}</tip>`;
    const complete = prior.startsWith("<domios ") ? prior : buildDomiosMessage(frame.attrs, frame.body, suffix);
    return complete.startsWith(echo);
  }
  const attrs = Object.entries(frame.attrs);
  return Object.keys(parsed.attrs).length === attrs.length
    && attrs.every(([name, value]) => parsed.attrs[name] === value)
    && userEchoBody(parsed.body) === userEchoBody(frame.body)
    && (parsed.content.slice(parsed.body.length).trim() || null) === (frame.tip == null ? null : `<tip>${frame.tip}</tip>`);
}

export function retainRoutedPresentation<T extends NormalizedChatMessage>(previous: T, incoming: T): T {
  const frame = previous.segments?.length === 1 ? previous.segments[0] : undefined;
  if (frame?.type !== "frame" || previous.type !== "user" || !canRetainRoutedPresentation(incoming)
    || previous.sourceSessionId !== incoming.sourceSessionId
    || previous.inheritedFromSessionId !== incoming.inheritedFromSessionId
    || (previous.correlationId && incoming.correlationId && previous.correlationId !== incoming.correlationId)
    || !matchesFrameCopy(incoming.content, previous.content, frame)) return incoming;

  return {
    ...incoming,
    content: previous.content,
    segments: previous.segments,
    segmentsVersion: previous.segmentsVersion,
    correlationId: incoming.correlationId ?? previous.correlationId,
  };
}
