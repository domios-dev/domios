import { useRef, useState, type ReactNode } from "react";
import { Maximize2 } from "lucide-react";
import { FullscreenOverlay, useFullscreenViewer } from "./FullscreenOverlay";

export function ExpandableReader({ label, enabled = true, onOpen, actionLabel = "Open full screen", children }: {
  label: string;
  enabled?: boolean;
  onOpen?: () => void;
  actionLabel?: string;
  children: (fullscreen: boolean) => ReactNode;
}) {
  const [open, setOpen] = useState(false);
  const panel = useRef<HTMLDivElement>(null);
  const opener = useRef<HTMLButtonElement>(null);
  const outer = useFullscreenViewer();
  return <>
    {enabled && <button ref={opener} type="button" hidden={open || Boolean(outer)} title={actionLabel === "Fullscreen" ? "Open in fullscreen" : undefined} style={{ display: open || outer ? "none" : undefined }} aria-label={`Open full screen: ${label}`}
      className="inline-flex min-h-11 items-center gap-1 rounded px-2 text-xs text-[var(--ct-accent)]"
      onClick={(event) => { event.currentTarget.focus(); onOpen?.(); setOpen(true); }}>
      <Maximize2 size={14} aria-hidden />{actionLabel}
    </button>}
    <FullscreenOverlay panelRef={panel} returnFocusRef={opener} open={open} label={label} onClose={() => setOpen(false)}>
      <div tabIndex={open ? 0 : undefined} role={open ? "region" : undefined} aria-label={open ? "Reader content" : undefined} style={open ? { overflow: "auto", minHeight: 0, flex: 1, color: "var(--ct-fg)" } : { display: "contents" }}>
        {children(open)}
      </div>
    </FullscreenOverlay>
  </>;
}
