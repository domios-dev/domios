import { attachmentPreviewKind, parseAttachmentMarkers, parseDomiosMessage, parseDsBlocks, type LocalArtifactKind, type MessageSegment } from "@codeterm/chat-core";

export interface ChatDocument { path: string; kind: LocalArtifactKind; name: string; shownAt: string | number | null; }
interface TranscriptMessage { content: string; timestamp?: string | number | null; type?: string; inboundKind?: string; segments?: readonly MessageSegment[] | null }

function untrustedSegments(segments: readonly MessageSegment[]): boolean {
  return segments.some((part) => part.type === "partial_frame" || part.type === "unknown_frame" || part.type === "paste"
    || (part.type === "frame" && (part.kind === "neighbor" || part.kind === "system_prompt" || untrustedSegments(part.segments ?? []))));
}

function displayedFrameBody(body: string): string {
  const trimmed = body.trimEnd();
  const first = trimmed.search(/\S/);
  return trimmed.slice(first < 0 ? trimmed.length : trimmed.lastIndexOf("\n", first) + 1);
}

function directivesOf(message: TranscriptMessage): { kind: LocalArtifactKind; path: string }[] {
  const routed = parseDomiosMessage(message.content);
  const frame = message.segments?.[0];
  const tailMatches = message.segments?.every((part, index) => index === 0 || (part.type === "text" && !part.text.trim()));
  const matchingHostSource = frame?.type === "frame" && tailMatches && message.inboundKind === frame.kind && frame.from === frame.kind && frame.attrs.from === frame.kind
    && (routed
      ? message.content.startsWith("<domios")
        && Object.keys(routed.attrs).length === Object.keys(frame.attrs).length
        && Object.entries(routed.attrs).every(([key, value]) => frame.attrs[key] === value)
        && displayedFrameBody(routed.body) === frame.body
      : displayedFrameBody(message.content) === frame.body);
  const claimsRoute = routed !== null || message.segments?.some((part) => part.type === "frame") || message.inboundKind !== undefined;
  const directMetadata = message.segments?.length && message.segments.every((part) => part.type === "text" || part.type === "attachment");
  const direct = !claimsRoute && (!message.content.startsWith("<domios") || Boolean(directMetadata));
  const trusted = Boolean(direct || matchingHostSource) && !untrustedSegments(message.segments ?? []) && !message.content.includes("<pasted_content");
  const source = matchingHostSource && routed ? routed.body : message.content;
  return parseDsBlocks(source, { id: "documents", trusted }).flatMap<{ kind: LocalArtifactKind; path: string }>((segment) => {
    if (segment.type !== "block") return [];
    const block = segment.block;
    if (block.kind === "page" || block.kind === "document") return [{ kind: block.kind, path: block.src }];
    return block.kind === "image" ? block.sources.map((path) => ({ kind: "image" as const, path })) : [];
  });
}

function attachmentsOf(content: string): { kind: LocalArtifactKind; path: string }[] {
  return parseAttachmentMarkers(content).attachments.flatMap((attachment) => {
    const kind = attachmentPreviewKind(attachment);
    return kind ? [{ kind, path: attachment.path }] : [];
  });
}

export function documentsFromTranscript(messages: readonly TranscriptMessage[]): ChatDocument[] {
  const documents = new Map<string, ChatDocument>();
  for (const message of messages) {
    const found = message.type === "user" ? [...directivesOf(message), ...attachmentsOf(message.content)] : directivesOf(message);
    for (const item of found) {
      const key = item.path.replace(/\\/g, "/");
      documents.delete(key);
      documents.set(key, { ...item, name: key.split("/").pop() ?? key, shownAt: message.timestamp ?? null });
    }
  }
  return [...documents.values()].reverse();
}
