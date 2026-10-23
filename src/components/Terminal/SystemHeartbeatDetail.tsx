import { Timer, X } from "lucide-react";
import { createPortal } from "react-dom";
import { formatClock, type DelayedMessage } from "./delayedSend";
import { presentSystemReminder } from "./systemReminderModel";
import { usePlansApiOptional, useSidebarPlansOptional } from "../Canvas/PlansProvider";
import { PlanModeControls } from "../Canvas/planView/PlanModeControls";

export function SystemHeartbeatDetail({ message, busy, onClose }: {
  message: DelayedMessage;
  busy: boolean;
  onClose: () => void;
}) {
  const api = usePlansApiOptional();
  const view = presentSystemReminder(message, Date.now(), formatClock);
  const plan = useSidebarPlansOptional().plans.find((candidate) => candidate.id === view.planId);
  const facts: Array<[string, string, string]> = [
    ["Why", view.why ?? "", "heartbeat-reason"],
    ["When", view.condition, "heartbeat-condition"],
    ["Next", view.nextText, "heartbeat-next"],
    ["Last sent", view.lastSent ?? "", "heartbeat-last-sent"],
    ["Last held", view.lastHeld ?? "", "heartbeat-last-held"],
  ];
  return createPortal(
    <div className="ct-delayed-modal__scrim" onMouseDown={(event) => {
      if (event.target === event.currentTarget && !busy) onClose();
    }}>
      <div className="ct-delayed-modal" role="dialog" aria-modal="true" aria-labelledby="system-heartbeat-title" data-testid="delayed-message-modal">
        <div className="ct-delayed-modal__head">
          <Timer size={14} aria-hidden="true" className="ct-delayed-modal__icon" />
          <span className="ct-system-heartbeat__cadence" data-testid="heartbeat-cadence">{view.cadence}</span>
          <h2 id="system-heartbeat-title">{view.label} · {view.title}</h2>
          <button type="button" className="ct-delayed-modal__close" aria-label="Close" disabled={busy} onClick={onClose}>
            <X size={14} aria-hidden="true" />
          </button>
        </div>
        <div className="ct-delayed-modal__body">
          <dl className="ct-delayed-modal__hint m-0 grid gap-1" aria-label="Reminder schedule">
            {facts.filter(([, value]) => value).map(([term, value, testId]) => (
              <div key={term}>
                <dt className="inline font-semibold">{term}: </dt>
                <dd className="m-0 inline" data-testid={testId}>{value}</dd>
              </div>
            ))}
          </dl>
          {view.sections.length > 0 && (
            <div className="ct-system-heartbeat__content" data-testid="heartbeat-content" tabIndex={0} role="region" aria-label="Reminder text">
              {view.sections.map((section, index) => (
                <section key={`${section.kind}:${index}`} aria-label={section.title || undefined}>
                  {section.title && <h3 className="m-0 font-semibold">{section.title}</h3>}
                  <p className="m-0 mb-2 whitespace-pre-wrap">{section.body}</p>
                </section>
              ))}
            </div>
          )}
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
