/** A reader's position expressed as content: the row's stable source key, its
 *  ordinal among rows sharing that key, and its offset from the viewport top. */
export interface ChatContentAnchor {
  key: string;
  ordinal: number;
  offset: number;
}

function renderedRows(el: HTMLElement): HTMLElement[] {
  return Array.from(el.querySelectorAll<HTMLElement>("[data-message-id]"));
}

// Render ids are range ordinals that change on re-key; the source id does not.
function rowKey(row: HTMLElement): string | undefined {
  return row.dataset.sourceMessageId ?? row.dataset.messageId;
}

/** Prefers the first row that starts inside the viewport, so growth of a
 *  partially visible row above never moves what the reader is looking at. */
export function captureChatContentAnchor(el: HTMLElement): ChatContentAnchor | null {
  const rows = renderedRows(el);
  const top = el.scrollTop;
  let anchorIndex = -1;
  for (let index = 0; index < rows.length; index += 1) {
    const rowTop = rows[index].offsetTop;
    if (rowTop >= top) {
      if (anchorIndex < 0 || rowTop < top + el.clientHeight) anchorIndex = index;
      break;
    }
    anchorIndex = index;
  }
  if (anchorIndex < 0) return null;
  const anchor = rows[anchorIndex];
  const key = rowKey(anchor);
  if (!key) return null;
  let ordinal = 0;
  for (let index = 0; index < anchorIndex; index += 1) {
    if (rowKey(rows[index]) === key) ordinal += 1;
  }
  return { key, ordinal, offset: anchor.offsetTop - top };
}

/** The scrollTop that puts the anchored row back at its offset; null when it is no longer rendered. */
export function chatScrollTopForAnchor(el: HTMLElement, anchor: ChatContentAnchor): number | null {
  let seen = 0;
  for (const row of renderedRows(el)) {
    if (rowKey(row) !== anchor.key) continue;
    if (seen === anchor.ordinal) return row.offsetTop - anchor.offset;
    seen += 1;
  }
  return null;
}
