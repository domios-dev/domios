import { useRef, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { FullscreenOverlay, useFullscreenKeyboard } from "@codeterm/chat-ui";
import { X } from "lucide-react";
import { IconAction } from "./PanelParts";

export function ChangeFullscreen({ path, onClose, children }: { path: string; onClose: () => void; children: ReactNode }) {
  const panelRef = useRef<HTMLDivElement>(null);
  useFullscreenKeyboard(true, panelRef, onClose);
  return createPortal(<FullscreenOverlay panelRef={panelRef} label={`Fullscreen changes for ${path}`} onClose={onClose}>
    <header className="tab-panel__bar"><span className="min-w-0 flex-1 truncate" title={path}>{path}</span>
      <IconAction label="Close fullscreen" icon={X} onClick={onClose} />
    </header>
    <div className="min-h-0 flex-1 overflow-auto p-4">{children}</div>
  </FullscreenOverlay>, document.body);
}
