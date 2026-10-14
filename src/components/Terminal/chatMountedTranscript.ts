import { chatAnchorIdentities, type ChatAnchorIdentity, type ChatContentAnchor } from "./chatAnchorModel";
import { IndexedChatRowGeometry } from "./chatRowGeometry";
import { CHAT_ROW_ESTIMATE } from "./chatMountedWindow";

type Message = { id: string; sourceMessageId?: string };

export interface ChatTranscriptIndex {
  messages: readonly Message[];
  geometry: IndexedChatRowGeometry;
  identities: ChatAnchorIdentity[];
  byId: Map<string, number>;
  bySource: Map<string, number>;
  byAnchor: Map<string, Map<number, number>>;
}

export function indexChatTranscript(messages: readonly Message[], heights: ReadonlyMap<string, number>, previous?: ChatTranscriptIndex): ChatTranscriptIndex {
  const appendOnly = previous && previous.messages.length <= messages.length
    && previous.messages.every((message, index) => message.id === messages[index].id);
  const geometry = appendOnly ? previous.geometry : new IndexedChatRowGeometry();
  for (let index = appendOnly ? previous.messages.length : 0; index < messages.length; index++) {
    geometry.append(heights.get(messages[index].id) ?? CHAT_ROW_ESTIMATE);
  }
  const identities = chatAnchorIdentities(messages);
  const byId = new Map(identities.map((identity, index) => [identity.id, index]));
  const bySource = new Map<string, number>();
  const byAnchor = new Map<string, Map<number, number>>();
  for (let index = 0; index < identities.length; index++) {
    const identity = identities[index];
    if (!bySource.has(identity.key)) bySource.set(identity.key, index);
    let ordinals = byAnchor.get(identity.key);
    if (!ordinals) { ordinals = new Map(); byAnchor.set(identity.key, ordinals); }
    ordinals.set(identity.ordinal, index);
  }
  return { messages, geometry, identities, byId, bySource, byAnchor };
}

export function indexedChatAnchor(index: ChatTranscriptIndex, anchor: ChatContentAnchor): number {
  return (anchor.id == null ? undefined : index.byId.get(anchor.id))
    ?? index.byAnchor.get(anchor.key)?.get(anchor.ordinal) ?? -1;
}
