import { parseLocalArtifactDirective, type LocalArtifactKind } from "@codeterm/chat-core";

export interface ChatDocument { path: string; kind: LocalArtifactKind; name: string; shownAt: string | number | null; }
export function documentsFromTranscript(messages: readonly { content: string; timestamp?: string | number | null }[]): ChatDocument[] {
  const documents = new Map<string, ChatDocument>();
  for (const message of messages) {
    let fence: string | null = null;
    for (const line of message.content.split(/\r?\n/)) {
      const marker = /^\s*(`{3,}|~{3,})/.exec(line)?.[1];
      if (marker) { if (!fence) fence = marker; else if (marker[0] === fence[0] && marker.length >= fence.length) fence = null; continue; }
      if (fence) continue;
      const directive = parseLocalArtifactDirective(line.replace(/^@ct-/, "@ds-"));
      if (!directive) continue;
      const key = directive.path.replace(/\\/g, "/");
      documents.delete(key);
      documents.set(key, { ...directive, name: key.split("/").pop() ?? key, shownAt: message.timestamp ?? null });
    }
  }
  return [...documents.values()].reverse();
}
