import { captureChatAnchor, chatAnchorIdentities, resolveChatAnchorTop, type ChatAnchorGeometry, type ChatContentAnchor } from "./chatAnchorModel";
export type { ChatContentAnchor } from "./chatAnchorModel";

function renderedRows(el: HTMLElement): ChatAnchorGeometry[] {
  const rows = Array.from(el.querySelectorAll<HTMLElement>("[data-message-id]"));
  const identities = chatAnchorIdentities(rows.map((row) => ({ id: row.dataset.messageId ?? "", sourceMessageId: row.dataset.sourceMessageId })));
  return rows.map((row, index) => ({ ...identities[index],
    ordinal: row.dataset.sourceMessageOrdinal == null ? identities[index].ordinal : Number(row.dataset.sourceMessageOrdinal),
    top: row.offsetTop }));
}

export function captureChatContentAnchor(el: HTMLElement): ChatContentAnchor | null {
  return captureChatAnchor(renderedRows(el), el.scrollTop);
}

export function chatScrollTopForAnchor(el: HTMLElement, anchor: ChatContentAnchor): number | null {
  return resolveChatAnchorTop(renderedRows(el), anchor);
}
