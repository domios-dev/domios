export interface ChatAnchorIdentity {
  id: string;
  key: string;
  ordinal: number;
}

export interface ChatContentAnchor {
  id?: string;
  key: string;
  ordinal: number;
  offset: number;
}

export interface ChatAnchorGeometry extends ChatAnchorIdentity { top: number }

export function chatAnchorIdentities(messages: readonly { id: string; sourceMessageId?: string }[]): ChatAnchorIdentity[] {
  const seen = new Map<string, number>();
  return messages.map(({ id, sourceMessageId }) => {
    const key = sourceMessageId ?? id;
    const ordinal = seen.get(key) ?? 0;
    seen.set(key, ordinal + 1);
    return { id, key, ordinal };
  });
}

export function captureChatAnchor(rows: readonly ChatAnchorGeometry[], scrollTop: number): ChatContentAnchor | null {
  const row = rows.find((candidate) => candidate.top >= scrollTop) ?? rows[rows.length - 1];
  return row ? { id: row.id, key: row.key, ordinal: row.ordinal, offset: row.top - scrollTop } : null;
}

export function resolveChatAnchorIndex(rows: readonly ChatAnchorIdentity[], anchor: ChatContentAnchor): number {
  const exact = anchor.id == null ? -1 : rows.findIndex((row) => row.id === anchor.id);
  return exact >= 0 ? exact : rows.findIndex((row) => row.key === anchor.key && row.ordinal === anchor.ordinal);
}

export function resolveChatAnchorTop(rows: readonly ChatAnchorGeometry[], anchor: ChatContentAnchor): number | null {
  const index = resolveChatAnchorIndex(rows, anchor);
  return index < 0 ? null : rows[index].top - anchor.offset;
}
