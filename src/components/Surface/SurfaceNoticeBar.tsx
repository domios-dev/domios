import { InlineErrorNotice } from "../shared/ErrorNotice";
import { X } from "lucide-react";
import type { SurfaceNotice } from "./surfaceStreamTransport";

export function SurfaceNoticeBar({ notice, onDismiss }: { notice: SurfaceNotice; onDismiss: () => void }) {
  const tone = notice.tone ?? "info";
  if (tone === "error") {
    return <div data-testid="surface-notice" data-tone={tone} className="ct-surface-notice">
      <InlineErrorNotice error={notice.text} operation="updating this surface" blocking
        actions={[{ label: "Dismiss", ariaLabel: "Dismiss notice", onClick: onDismiss }]} />
    </div>;
  }
  return (
    <div data-testid="surface-notice" data-tone={tone} role="status" aria-label={notice.text} className="ct-surface-notice">
      <span className="ct-surface-notice__text">{notice.text}</span>
      <button type="button" className="ct-surface-icon-btn ct-liquid-interactive" aria-label="Dismiss notice" title="Dismiss" onClick={onDismiss}>
        <X size={14} aria-hidden="true" />
      </button>
    </div>
  );
}
