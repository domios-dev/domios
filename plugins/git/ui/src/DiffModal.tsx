import { useEffect } from "react";
import { createPortal } from "react-dom";
import { Columns2, Rows3, X } from "lucide-react";
import type { ApiConfig } from "./types";
import { DiffPane, type DisplayFile } from "./CommitDetail";
import { UI_BUTTON } from "./design";

interface DiffModalProps {
  api: ApiConfig;
  cwd: string;
  sha: string;
  file: DisplayFile;
  split: boolean;
  onToggleSplit: () => void;
  onClose: () => void;
  fullscreen?: boolean;
}

// Overlay diff view used by the medium (windowed) and narrow (fullscreen)
// layouts so the diff experience is one component. Mounts via portal.
export function DiffModal({
  api,
  cwd,
  sha,
  file,
  split,
  onToggleSplit,
  onClose,
  fullscreen,
}: DiffModalProps) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        onClose();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [onClose]);

  const node = (
    <div
      className={`ct-git-diffmodal-overlay${fullscreen ? " is-fullscreen" : ""}`}
      onPointerDown={onClose}
    >
      <div className="ct-git-diffmodal" onPointerDown={(e) => e.stopPropagation()}>
        <div className="ct-git-diffmodal-head">
          <span className="path" title={file.path}>
            {file.old_path ? `${file.old_path} → ${file.path}` : file.path}
          </span>
          <span className="stats">
            <span className="add">+{file.added ?? "–"}</span>
            <span className="del">−{file.deleted ?? "–"}</span>
          </span>
          {!fullscreen && (
            <button
              type="button"
              onClick={onToggleSplit}
              title={split ? "Unified view" : "Split view"}
              className={`${UI_BUTTON.icon} ${UI_BUTTON.sm} shrink-0`}
            >
              {split ? <Rows3 size={14} /> : <Columns2 size={14} />}
            </button>
          )}
          <button
            type="button"
            onClick={onClose}
            title="Close (Esc)"
            className={`${UI_BUTTON.icon} ${UI_BUTTON.sm} shrink-0`}
          >
            <X size={14} />
          </button>
        </div>
        <div className="ct-git-diffmodal-body">
          <DiffPane api={api} cwd={cwd} sha={sha} path={file.path} split={fullscreen ? false : split} />
        </div>
      </div>
    </div>
  );

  return createPortal(node, document.body);
}
