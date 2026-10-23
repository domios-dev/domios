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

/** `top`/`height` frame the whole row for visibility; `contentTop` is the message body, which a day separator above it never shifts. */
export interface ChatAnchorGeometry extends ChatAnchorIdentity { top: number; height?: number; contentTop?: number }

function anchorTop(row: ChatAnchorGeometry): number {
  return row.contentTop ?? row.top;
}

export function chatVisibleAnchorIndex(rows: readonly { top: number; height?: number }[], scrollTop: number): number {
  const index = rows.findIndex((row, index) => (row.height == null ? rows[index + 1]?.top ?? Infinity : row.top + row.height) > scrollTop);
  return index >= 0 ? index : rows.length - 1;
}

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
  const row = rows[chatVisibleAnchorIndex(rows, scrollTop)];
  return row ? { id: row.id, key: row.key, ordinal: row.ordinal, offset: anchorTop(row) - scrollTop } : null;
}

export function resolveChatAnchorIndex(rows: readonly ChatAnchorIdentity[], anchor: ChatContentAnchor): number {
  const exact = anchor.id == null ? -1 : rows.findIndex((row) => row.id === anchor.id);
  return exact >= 0 ? exact : rows.findIndex((row) => row.key === anchor.key && row.ordinal === anchor.ordinal);
}

export function resolveChatAnchorTop(rows: readonly ChatAnchorGeometry[], anchor: ChatContentAnchor): number | null {
  const index = resolveChatAnchorIndex(rows, anchor);
  return index < 0 ? null : anchorTop(rows[index]) - anchor.offset;
}

export function chatRevealIndex(messages: readonly { id: string; sourceMessageId?: string }[], messageId: string): number {
  const source = messages.findIndex((message) => (message.sourceMessageId ?? message.id) === messageId);
  return source >= 0 ? source : messages.findIndex((message) => message.id === messageId);
}
