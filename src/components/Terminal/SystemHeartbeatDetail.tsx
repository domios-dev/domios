import { Timer, X } from "lucide-react";
import { createPortal } from "react-dom";
import type { DelayedMessage } from "./delayedSend";
import { formatCadence } from "./systemHeartbeatProjection";
import { usePlansApiOptional, useSidebarPlansOptional } from "../Canvas/PlansProvider";
import { PlanModeControls } from "../Canvas/planView/PlanModeControls";

/** Heartbeat ids are `system:heartbeat:<plan>:<tab>`. */
function heartbeatPlanId(id: string): string | null {
  const [scope, kind, planId] = id.split(":");
  return scope === "system" && kind === "heartbeat" && planId ? planId : null;
}

export function SystemHeartbeatDetail({ message, busy, onClose }: {
  message: DelayedMessage;
  busy: boolean;
  onClose: () => void;
}) {
  const api = usePlansApiOptional();
  const planId = heartbeatPlanId(message.id);
  const plan = useSidebarPlansOptional().plans.find((candidate) => candidate.id === planId);
  return createPortal(
    <div className="ct-delayed-modal__scrim" onMouseDown={(event) => {
      if (event.target === event.currentTarget && !busy) onClose();
    }}>
      <div className="ct-delayed-modal" role="dialog" aria-modal="true" aria-labelledby="system-heartbeat-title" data-testid="delayed-message-modal">
        <div className="ct-delayed-modal__head">
          <Timer size={14} aria-hidden="true" className="ct-delayed-modal__icon" />
          <span className="ct-system-heartbeat__cadence" data-testid="heartbeat-cadence">{formatCadence(message.recurrence?.everyMs ?? 0)}</span>
          <h2 id="system-heartbeat-title">System heartbeat</h2>
          <button type="button" className="ct-delayed-modal__close" aria-label="Close" disabled={busy} onClick={onClose}>
            <X size={14} aria-hidden="true" />
          </button>
        </div>
        <div className="ct-delayed-modal__body">
          <p className="ct-delayed-modal__hint" data-testid="heartbeat-reason">{message.reason}</p>
          <pre className="ct-system-heartbeat__content" data-testid="heartbeat-content" tabIndex={0} aria-label="Message that will be sent">{message.text}</pre>
        </div>
        <div className="ct-delayed-modal__foot">
          {plan ? <PlanModeControls plan={plan} api={api} reason="Paused from its heartbeat" hint /> : <span className="ct-delayed-modal__spacer" />}
          <button type="button" className="ct-delayed-modal__btn" disabled={busy} onClick={onClose}>Close</button>
        </div>
      </div>
    </div>,
    document.body,
  );
}
