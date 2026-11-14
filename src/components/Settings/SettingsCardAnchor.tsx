import { useEffect, useRef, type ReactNode } from "react";
import type { SettingsCardAnchorId } from "./sections";

const CARD_SCROLL_MARGIN = 16;

/**
 * Scrolls only the card's nearest scrolling ancestor. scrollIntoView also scrolls overflow-hidden ancestors, and a
 * last card that cannot reach the top shifted the whole Settings layer up, leaving a blank band below it.
 */
function scrollToTopOfOwnScroller(card: HTMLElement) {
  const view = card.ownerDocument.defaultView;
  if (!view) return;
  let scroller = card.parentElement;
  while (scroller && !/(auto|scroll)/.test(view.getComputedStyle(scroller).overflowY)) scroller = scroller.parentElement;
  if (!scroller) return;
  const offset = card.getBoundingClientRect().top - scroller.getBoundingClientRect().top;
  scroller.scrollTop = Math.max(0, scroller.scrollTop + offset - CARD_SCROLL_MARGIN);
}

/** A deep-link landing spot: when focused, the card scrolls into view and takes focus for keyboard users. */
export function SettingsCardAnchor({
  id,
  focused,
  focusRequest = 0,
  children,
}: {
  id: SettingsCardAnchorId;
  focused: boolean;
  /** Bump to land on the card again while it is already shown. */
  focusRequest?: number;
  children: ReactNode;
}) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!focused && !focusRequest) return;
    if (ref.current) scrollToTopOfOwnScroller(ref.current);
    ref.current?.focus({ preventScroll: true });
  }, [focused, focusRequest]);

  return (
    <div
      ref={ref}
      tabIndex={-1}
      data-settings-card={id}
      data-settings-card-focused={focused || undefined}
      className="rounded-xl outline-none focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--ct-accent)]"
    >
      {children}
    </div>
  );
}
