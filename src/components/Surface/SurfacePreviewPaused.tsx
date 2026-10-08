import { useId, useState } from "react";
import type { SurfaceNotice } from "./surfaceStreamTransport";

export const PREVIEW_PAUSED_TEXT = "Preview paused while the window is hidden";

export function SurfacePreviewPaused({ notice }: { notice: SurfaceNotice }) {
  const [open, setOpen] = useState(false);
  const detailId = useId();
  const { detail } = notice;
  return (
    <div role="status" data-testid="surface-preview-paused" className="ct-surface-paused">
      <span className="ct-surface-paused__text">{PREVIEW_PAUSED_TEXT}</span>
      {detail && (
        <>
          <button
            type="button"
            className="ct-surface-paused__toggle"
            aria-expanded={open}
            aria-controls={detailId}
            onClick={() => setOpen((value) => !value)}
          >
            Details
          </button>
          {open && <span id={detailId} className="ct-surface-paused__detail">{detail}</span>}
        </>
      )}
    </div>
  );
}
