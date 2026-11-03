import { createContext, useContext, useRef, useState, type ReactNode } from "react";
import { Maximize2 } from "lucide-react";
import { FullscreenOverlay, useFullscreenViewer } from "./FullscreenOverlay";

const ReaderHostContext = createContext(false);

export function useReaderHost() {
  return useContext(ReaderHostContext);
}

export function ExpandableReader({ label, enabled = true, onOpen, openerPlacement = "inline", children }: {
  label: string;
  enabled?: boolean;
  onOpen?: () => void;
  openerPlacement?: "inline" | "header";
  children: (fullscreen: boolean, opener: ReactNode) => ReactNode;
}) {
  const [open, setOpen] = useState(false);
  const panel = useRef<HTMLDivElement>(null);
  const opener = useRef<HTMLButtonElement>(null);
  const outer = useFullscreenViewer();
  const parentReader = useReaderHost();
  const control = enabled && <button ref={opener} type="button" hidden={open || Boolean(outer)} title="Open full screen" style={{ display: open || outer ? "none" : undefined }} aria-label={`Open full screen: ${label}`}
    className={openerPlacement === "header" ? "ct-reader-opener" : "ct-reader-opener ct-reader-opener--inline"}
    onClick={(event) => { event.stopPropagation(); event.currentTarget.focus(); onOpen?.(); setOpen(true); }}>
    <Maximize2 size={14} aria-hidden />{openerPlacement === "inline" && <span>Full screen</span>}
  </button>;
  return <>
    {openerPlacement === "inline" && control}
    <FullscreenOverlay panelRef={panel} returnFocusRef={opener} open={open} label={label} onClose={() => setOpen(false)}>
      <div tabIndex={open ? 0 : undefined} role={open ? "region" : undefined} aria-label={open ? "Reader content" : undefined} style={open ? { overflow: "auto", minHeight: 0, flex: 1, color: "var(--ct-fg)" } : { display: "contents" }}>
        <ReaderHostContext.Provider value={enabled || parentReader}>
          {children(open, openerPlacement === "header" ? control : null)}
        </ReaderHostContext.Provider>
      </div>
    </FullscreenOverlay>
  </>;
}
