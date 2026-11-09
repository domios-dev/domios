import { useId } from "react";
import { useChatBlockHost, type ChatCardMenuSpec } from "./blocks/ChatBlockHost";
import { writeClipboardFlavours } from "./capsuleClipboard";
import { useFullscreenViewer } from "./FullscreenOverlay";
import { mergeMessageMenu, useRowMenu } from "./messageMenu";
import { useCompactCard } from "./useCompactToolRow";

/**
 * A card's quiet overflow menu; renders nothing without a host menu or items.
 * With `joinRow`, a card that speaks for its chat row shares one menu with the message: on desktop its items
 * move into the row's side-dot menu and no ⋮ is drawn; on a compact touch surface this ⋮ shows the merged list.
 */
export function ChatCardMenu({ label, items, joinRow = false }: ChatCardMenuSpec & { joinRow?: boolean }) {
  const host = useChatBlockHost();
  const key = useId();
  const { compact } = useCompactCard();
  const inReader = Boolean(useFullscreenViewer());
  const row = useRowMenu(key, items, joinRow && !inReader);
  if (!host.renderMenu) return null;
  if (row && !compact) return null;
  const shown = row ? mergeMessageMenu(row.messageItems(), items) : items;
  if (shown.length === 0) return null;
  return <>{host.renderMenu({ label, items: shown })}</>;
}

export function copyPlainText(text: string): void {
  writeClipboardFlavours({ plain: text, portable: text });
}

const ASK_EXCERPT_LIMIT = 4000;

export function askExcerpt(text: string): string {
  const trimmed = text.trim();
  return trimmed.length > ASK_EXCERPT_LIMIT ? `${trimmed.slice(0, ASK_EXCERPT_LIMIT)}…` : trimmed;
}
