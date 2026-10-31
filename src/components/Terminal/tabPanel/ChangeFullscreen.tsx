import { useRef, type ReactNode } from "react";
import { FullscreenOverlay } from "@codeterm/chat-ui";

export function ChangeFullscreen({ path, open, onClose, children }: { path: string; open: boolean; onClose: () => void; children: ReactNode }) {
  const panelRef = useRef<HTMLDivElement>(null);
  return <FullscreenOverlay panelRef={panelRef} open={open} label={`Fullscreen changes for ${path}`} onClose={onClose}
    header={open && <span className="min-w-0 flex-1 truncate text-sm" title={path}>{path}</span>}>
    <div className={open ? "min-h-0 flex-1 overflow-auto p-4" : undefined}>{children}</div>
  </FullscreenOverlay>;
}
