import { useRef, type RefObject } from "react";
import { createPortal } from "react-dom";
import { Film, FolderOpen, X } from "lucide-react";
import { FullscreenOverlay, useFullscreenHostBounds, useFullscreenKeyboard } from "./FullscreenOverlay";
import { localArtifactOpeningAvailable } from "./localArtifactLoader";
import { revealLocalFile } from "./ImageFullscreen";
import { useLocalVideo } from "./localVideo";

const ACTION = "ct-liquid-interactive inline-flex h-7 shrink-0 items-center gap-1.5 rounded-md border px-2 text-[11px] hover:bg-[var(--ct-surface)] focus-visible:outline focus-visible:outline-1 focus-visible:outline-[var(--ct-accent)]";
const ACTION_STYLE = { borderColor: "var(--ct-border)", color: "var(--ct-fg)" };

export function VideoFullscreen({ label, path, anchorRef, onClose }: {
  label: string;
  path: string;
  anchorRef: RefObject<HTMLElement | null>;
  onClose: () => void;
}) {
  const panelRef = useRef<HTMLDivElement>(null);
  useFullscreenKeyboard(true, panelRef, onClose);
  const { bounds } = useFullscreenHostBounds(true, anchorRef);
  const video = useLocalVideo(path, true);
  return createPortal(
    <FullscreenOverlay panelRef={panelRef} label={`Video ${label}`} onClose={onClose} style={bounds}>
      <header className="flex min-w-0 items-center gap-2 px-1.5 pb-2 text-xs" style={{ color: "var(--ct-muted-fg)" }}>
        <Film size={14} aria-hidden className="shrink-0" />
        <span className="shrink-0 font-medium" style={{ color: "var(--ct-fg)" }}>{label}</span>
        <span className="min-w-0 flex-1 truncate font-mono text-[11px] opacity-70" title={path}>{path}</span>
        {localArtifactOpeningAvailable() && (
          <button type="button" className={ACTION} style={ACTION_STYLE} onClick={() => revealLocalFile(path)} aria-label={`Reveal ${label}`} title="Reveal in file manager">
            <FolderOpen size={13} aria-hidden />
            Reveal
          </button>
        )}
        <button type="button" className={ACTION} style={ACTION_STYLE} onClick={onClose} aria-label="Close video">
          <X size={13} aria-hidden />
        </button>
      </header>
      <div className="flex min-h-0 flex-1 items-center justify-center overflow-auto rounded-md p-4" style={{ background: "color-mix(in srgb, var(--ct-bg) 82%, black)" }} onClick={onClose}>
        {video.src
          ? <video src={video.src} controls autoPlay playsInline aria-label={label} className="max-h-full max-w-full rounded-md shadow-2xl" onClick={(event) => event.stopPropagation()} />
          : <span role={video.failed ? "alert" : "status"} style={{ color: "var(--ct-muted-fg)" }}>{video.failed ? "Video unavailable (over 48 MB or unreadable); reveal the file to play it" : "Loading video…"}</span>}
      </div>
    </FullscreenOverlay>,
    document.body,
  );
}
