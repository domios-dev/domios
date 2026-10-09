import { useChatBlockHost, type ChatCardMenuSpec } from "./blocks/ChatBlockHost";
import { writeClipboardFlavours } from "./capsuleClipboard";

/** A card's quiet overflow menu; renders nothing without a host menu or items. */
export function ChatCardMenu({ label, items }: ChatCardMenuSpec) {
  const host = useChatBlockHost();
  if (!host.renderMenu || items.length === 0) return null;
  return <>{host.renderMenu({ label, items })}</>;
}

export function copyPlainText(text: string): void {
  writeClipboardFlavours({ plain: text, portable: text });
}

const ASK_EXCERPT_LIMIT = 4000;

export function askExcerpt(text: string): string {
  const trimmed = text.trim();
  return trimmed.length > ASK_EXCERPT_LIMIT ? `${trimmed.slice(0, ASK_EXCERPT_LIMIT)}…` : trimmed;
}
