import { InlineErrorNotice } from "../shared/ErrorNotice";
import { X } from "lucide-react";
import type { SurfaceNotice } from "./surfaceStreamTransport";

export function SurfaceNoticeBar({ notice, onDismiss }: { notice: SurfaceNotice; onDismiss: () => void }) {
  const tone = notice.tone ?? "info";
  return (
    <div data-testid="surface-notice" data-tone={tone} className="ct-surface-notice">
      {tone === "error" ? <InlineErrorNotice error={notice.text} operation="updating this surface" blocking /> : <span role="status" className="ct-surface-notice__text">{notice.text}</span>}
      <button type="button" className="ct-surface-icon-btn ct-liquid-interactive" aria-label="Dismiss notice" title="Dismiss" onClick={onDismiss}>
        <X size={14} aria-hidden="true" />
      </button>
    </div>
  );
}
