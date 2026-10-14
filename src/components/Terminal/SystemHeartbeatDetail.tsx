import { Timer, X } from "lucide-react";
import { createPortal } from "react-dom";
import type { DelayedMessage } from "./delayedSend";
import { formatCadence, formatTickAt } from "./systemHeartbeatProjection";

export function SystemHeartbeatDetail({ message, busy, onClose }: {
  message: DelayedMessage;
  busy: boolean;
  onClose: () => void;
}) {
  const ticks = message.ticks ?? [];
  return createPortal(
    <div className="ct-delayed-modal__scrim" onMouseDown={(event) => {
      if (event.target === event.currentTarget && !busy) onClose();
    }}>
      <div className="ct-delayed-modal" role="dialog" aria-modal="true" aria-labelledby="system-heartbeat-title" data-testid="delayed-message-modal">
        <div className="ct-delayed-modal__head">
          <Timer size={14} aria-hidden="true" className="ct-delayed-modal__icon" />
          <h2 id="system-heartbeat-title">System heartbeat</h2>
          <button type="button" className="ct-delayed-modal__close" aria-label="Close" disabled={busy} onClick={onClose}>
            <X size={14} aria-hidden="true" />
          </button>
        </div>
        <div className="ct-delayed-modal__body">
          <p data-testid="heartbeat-origin">system</p>
          <p data-testid="heartbeat-cadence">{formatCadence(message.recurrence?.everyMs ?? 0)}</p>
          <p data-testid="heartbeat-reason">{message.reason}</p>
          <p data-testid="heartbeat-content">{message.text}</p>
          <ul>
            {ticks.map((tick) => (
              <li key={`${tick.atMs}:${tick.spinalReason}`} data-testid="heartbeat-tick">
                <time dateTime={new Date(tick.atMs).toISOString()}>{formatTickAt(tick.atMs)}</time>
                <span>{tick.outcome}</span>
                <span>{tick.spinalVerdict}</span>
                <span>{tick.spinalReason}</span>
              </li>
            ))}
          </ul>
        </div>
        <div className="ct-delayed-modal__foot">
          <span className="ct-delayed-modal__spacer" />
          <button type="button" className="ct-delayed-modal__btn" disabled={busy} onClick={onClose}>Close</button>
        </div>
      </div>
    </div>,
    document.body,
  );
}
