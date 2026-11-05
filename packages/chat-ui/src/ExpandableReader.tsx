import { createContext, useContext, useRef, useState, type ReactNode } from "react";
import { Maximize2 } from "lucide-react";
import { FullscreenOverlay, useFullscreenViewer } from "./FullscreenOverlay";
import type { ChatCardMenuItem } from "./blocks/ChatBlockHost";
import { ChatCardMenu } from "./ChatCardMenu";
import { useCompactCard } from "./useCompactToolRow";

const ReaderHostContext = createContext(false);

export function useReaderHost() {
  return useContext(ReaderHostContext);
}

/**
 * A card that can open in a full-screen reader. With a header opener on a compact touch surface
 * (see `useCompactCard`), "Open full screen" moves under ⋮: a card that renders its own menu
 * (`ownsMenu`) puts the given `readerItems` first in it; any other card gets a ⋮ in the opener's place.
 */
export function ExpandableReader({ label, enabled = true, onOpen, openerPlacement = "inline", ownsMenu = false, children }: {
  label: string;
  enabled?: boolean;
  onOpen?: () => void;
  openerPlacement?: "inline" | "header";
  ownsMenu?: boolean;
  children: (fullscreen: boolean, opener: ReactNode, openReader: (target?: HTMLElement | null) => void, readerItems: readonly ChatCardMenuItem[]) => ReactNode;
}) {
  const [open, setOpen] = useState(false);
  const panel = useRef<HTMLDivElement>(null);
  const opener = useRef<HTMLButtonElement>(null);
  const openReader = (target?: HTMLElement | null) => {
    target?.focus();
    onOpen?.();
    setOpen(true);
  };
  const outer = useFullscreenViewer();
  const parentReader = useReaderHost();
  const inMenu = useCompactCard().compact && openerPlacement === "header";
  // A card with its own menu always lists the reader, so a row menu that merges it (see `mergeMessageMenu`) can open it.
  // A menu item runs after the menu has closed and focused its trigger again, so that trigger is where focus returns.
  const readerItems: ChatCardMenuItem[] = (inMenu || ownsMenu) && enabled && !open && !outer ? [{
    label: "Open full screen",
    kind: "open",
    icon: <Maximize2 size={14} aria-hidden />,
    testId: "chat-reader-open",
    onSelect: () => openReader(document.activeElement instanceof HTMLElement ? document.activeElement : null),
  }] : [];
  const control = inMenu
    ? !ownsMenu && <ChatCardMenu label={`Actions for ${label}`} items={readerItems} />
    : enabled && <button ref={opener} type="button" hidden={open || Boolean(outer)} title="Open full screen" style={{ display: open || outer ? "none" : undefined }} aria-label={`Open full screen: ${label}`}
      className={openerPlacement === "header" ? "ct-reader-opener" : "ct-reader-opener ct-reader-opener--inline"}
      onClick={(event) => { event.stopPropagation(); openReader(event.currentTarget); }}>
      <Maximize2 size={14} aria-hidden />{openerPlacement === "inline" && <span>Full screen</span>}
    </button>;
  return <>
    <FullscreenOverlay panelRef={panel} returnFocusRef={opener} open={open} label={label} onClose={() => setOpen(false)}>
      <div tabIndex={open ? 0 : undefined} role={open ? "region" : undefined} aria-label={open ? "Reader content" : undefined} style={open ? { overflow: "auto", minHeight: 0, flex: 1, color: "var(--ct-fg)" } : { display: "contents" }}>
        <ReaderHostContext.Provider value={enabled || parentReader}>
          {children(open, openerPlacement === "header" ? control : null, openReader, readerItems)}
        </ReaderHostContext.Provider>
      </div>
    </FullscreenOverlay>
    {openerPlacement === "inline" && control}
  </>;
}
