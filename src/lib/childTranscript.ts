import type { NormalizedChatMessage } from "@codeterm/chat-core";

export interface ChildTranscriptAuthority { rootThreadId: string; bindingNonce: string; epoch: number }
export interface ChildMetadata {
  threadId: string;
  parentThreadId: string;
  ordinal: number;
  status: string;
  prompt: string | null;
  model: string | null;
  effort: string | null;
}
export interface ChildTranscriptPage extends ChildTranscriptAuthority {
  child: ChildMetadata;
  items: NormalizedChatMessage[];
  nextCursor: string | null;
}

export function childHistorySuffix(threadId: string, request: ChildTranscriptAuthority & {cursor?: string}): string {
  const query = new URLSearchParams({ rootThreadId: request.rootThreadId, bindingNonce: request.bindingNonce, epoch: String(request.epoch) });
  if (request.cursor) query.set("cursor", request.cursor);
  return `/agent/runtime/children/${encodeURIComponent(threadId)}/history?${query}`;
}

export function childTranscriptKey(tab: string, authority: ChildTranscriptAuthority): string {
  return JSON.stringify([tab, authority.rootThreadId, authority.bindingNonce, authority.epoch]);
}

export function acceptChildPage(authority: ChildTranscriptAuthority, thread: string, page: ChildTranscriptPage): boolean {
  return page.rootThreadId === authority.rootThreadId && page.bindingNonce === authority.bindingNonce
    && page.epoch === authority.epoch && page.child.threadId === thread && thread !== authority.rootThreadId;
}

export function prependChildPage(current: NormalizedChatMessage[], older: NormalizedChatMessage[]): NormalizedChatMessage[] {
  const ids = new Set(current.map(item => item.id));
  return [...older.filter(item => !ids.has(item.id)), ...current];
}
