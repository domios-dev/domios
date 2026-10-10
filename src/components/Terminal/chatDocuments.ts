import { attachmentPreviewKind, parseAttachmentMarkers, parseLocalArtifactDirective, type LocalArtifactKind } from "@codeterm/chat-core";

export interface ChatDocument { path: string; kind: LocalArtifactKind; name: string; shownAt: string | number | null; }
interface TranscriptMessage { content: string; timestamp?: string | number | null; type?: string }

function directivesOf(content: string): { kind: LocalArtifactKind; path: string }[] {
  const found: { kind: LocalArtifactKind; path: string }[] = [];
  let fence: string | null = null;
  for (const line of content.split(/\r?\n/)) {
    const marker = /^\s*(`{3,}|~{3,})/.exec(line)?.[1];
    if (marker) { if (!fence) fence = marker; else if (marker[0] === fence[0] && marker.length >= fence.length) fence = null; continue; }
    if (fence) continue;
    const directive = parseLocalArtifactDirective(line.replace(/^@ct-/, "@ds-"));
    if (directive) found.push(directive);
  }
  return found;
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
    const found = message.type === "user" ? [...directivesOf(message.content), ...attachmentsOf(message.content)] : directivesOf(message.content);
    for (const item of found) {
      const key = item.path.replace(/\\/g, "/");
      documents.delete(key);
      documents.set(key, { ...item, name: key.split("/").pop() ?? key, shownAt: message.timestamp ?? null });
    }
  }
  return [...documents.values()].reverse();
}
