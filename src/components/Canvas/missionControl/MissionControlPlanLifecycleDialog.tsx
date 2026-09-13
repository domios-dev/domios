import { useEffect, useRef, useState, type KeyboardEvent, type MouseEvent } from "react";
import { createPortal } from "react-dom";
import {
  PLAN_PAUSE_EFFECT_LABEL,
  PLAN_PAUSE_ROLE_LABEL,
  type PlanLifecycleConfirm,
  type PlanLifecycleKind,
  type PlanLifecyclePreview,
  type PlanLifecyclePreviewLoader,
} from "../planLifecycle";

export interface MissionControlPlanLifecycleDialogProps {
  kind: PlanLifecycleKind;
  planId: string;
  loadPreview?: PlanLifecyclePreviewLoader;
  onConfirm?: PlanLifecycleConfirm;
  onClose: () => void;
}

type PreviewState =
  | { status: "loading" }
  | { status: "ready"; preview: PlanLifecyclePreview }
  | { status: "error"; message: string };

const FOCUSABLE = 'button:not([disabled]), [href], input:not([disabled]), [tabindex]:not([tabindex="-1"])';

const COPY: Record<PlanLifecycleKind, { title: string; confirm: string; agents: string; schedules: string; effects: string }> = {
  pause: {
    title: "Pause plan",
    confirm: "Pause",
    agents: "Notified to pause",
    schedules: "Schedules disabled",
    effects: "Silenced",
  },
  resume: {
    title: "Resume plan",
    confirm: "Resume",
    agents: "Notified to continue",
    schedules: "Schedules restarted",
    effects: "Restored",
  },
};

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export function MissionControlPlanLifecycleDialog({
  kind,
  planId,
  loadPreview,
  onConfirm,
  onClose,
}: MissionControlPlanLifecycleDialogProps) {
  const dialogRef = useRef<HTMLDivElement | null>(null);
  const loaderRef = useRef(loadPreview);
  loaderRef.current = loadPreview;
  const [state, setState] = useState<PreviewState>({ status: "loading" });
  const [submitting, setSubmitting] = useState(false);
  const [submitError, setSubmitError] = useState<string | null>(null);
  const copy = COPY[kind];
  const testId = `mission-control-plan-${kind}-dialog`;
  const titleId = `${testId}-title`;
  const summaryId = `${testId}-summary`;

  useEffect(() => {
    let active = true;
    setState({ status: "loading" });
    const loader = loaderRef.current;
    if (!loader) {
      setState({ status: "error", message: "Preview unavailable" });
      return;
    }
    loader(planId, kind)
      .then((preview) => {
        if (active) setState({ status: "ready", preview });
      })
      .catch((error: unknown) => {
        if (active) setState({ status: "error", message: errorMessage(error) });
      });
    return () => {
      active = false;
    };
  }, [planId, kind]);

  useEffect(() => {
    dialogRef.current?.querySelector<HTMLElement>("[data-autofocus]")?.focus();
  }, []);

  const preview = state.status === "ready" ? state.preview : null;
  const canConfirm = Boolean(preview?.applicable && onConfirm && !submitting);

  const cancel = () => {
    if (!submitting) onClose();
  };

  const confirm = async () => {
    if (!canConfirm || !onConfirm) return;
    setSubmitting(true);
    setSubmitError(null);
    try {
      await onConfirm(planId, kind);
      onClose();
    } catch (error) {
      setSubmitError(errorMessage(error));
      setSubmitting(false);
    }
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    event.stopPropagation();
    if (event.key === "Escape") {
      event.preventDefault();
      cancel();
      return;
    }
    if (event.key !== "Tab") return;
    const focusable = Array.from(dialogRef.current?.querySelectorAll<HTMLElement>(FOCUSABLE) ?? []);
    if (focusable.length === 0) {
      event.preventDefault();
      return;
    }
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    const active = document.activeElement;
    if (event.shiftKey && (active === first || !dialogRef.current?.contains(active))) {
      event.preventDefault();
      last?.focus();
    } else if (!event.shiftKey && (active === last || !dialogRef.current?.contains(active))) {
      event.preventDefault();
      first?.focus();
    }
  };

  const onBackdropMouseDown = (event: MouseEvent<HTMLDivElement>) => {
    if (event.target === event.currentTarget) cancel();
  };

  return createPortal(
    <div className="mc-lifecycle-backdrop" data-testid={`${testId}-backdrop`} onMouseDown={onBackdropMouseDown}>
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        aria-describedby={summaryId}
        aria-busy={state.status === "loading" || submitting}
        data-testid={testId}
        data-plan-id={planId}
        className="mc-lifecycle-dialog"
        onKeyDown={onKeyDown}
        onClick={(event) => event.stopPropagation()}
      >
        <h2 id={titleId} className="mc-lifecycle-title">
          {copy.title}
        </h2>
        <p id={summaryId} className="mc-lifecycle-meta">
          {state.status === "loading" && "Loading preview"}
          {state.status === "error" && "Preview unavailable"}
          {preview && (preview.plan_title || preview.plan_id)}
        </p>
        {state.status === "error" && (
          <p role="alert" className="mc-lifecycle-error">
            {state.message}
          </p>
        )}
        {preview && !preview.applicable && (
          <p role="status" className="mc-lifecycle-meta" data-testid={`${testId}-not-applicable`}>
            {preview.not_applicable_reason ?? "Nothing to change"}
          </p>
        )}
        {preview?.applicable && (
          <>
            <section className="mc-lifecycle-section" aria-label={copy.agents}>
              <h3>{copy.agents}</h3>
              {preview.agents.length === 0 ? (
                <p className="mc-lifecycle-meta">No live agents</p>
              ) : (
                <ul className="mc-lifecycle-list" data-testid={`${testId}-agents`}>
                  {preview.agents.map((agent) => (
                    <li
                      key={agent.tab_id}
                      data-testid="mission-control-plan-lifecycle-agent"
                      data-tab-id={agent.tab_id}
                      data-available={agent.available ? "true" : "false"}
                    >
                      <span className="mc-lifecycle-name">{agent.name}</span>
                      <span className="mc-lifecycle-tag">{PLAN_PAUSE_ROLE_LABEL[agent.role]}</span>
                      {!agent.available && <span className="mc-lifecycle-unavailable">Unavailable</span>}
                    </li>
                  ))}
                </ul>
              )}
            </section>
            <section className="mc-lifecycle-section" aria-label={copy.schedules}>
              <h3>{copy.schedules}</h3>
              {preview.schedules.length === 0 ? (
                <p className="mc-lifecycle-meta">None</p>
              ) : (
                <ul className="mc-lifecycle-list" data-testid={`${testId}-schedules`}>
                  {preview.schedules.map((schedule) => (
                    <li
                      key={schedule.job_id}
                      data-testid="mission-control-plan-lifecycle-schedule"
                      data-job-id={schedule.job_id}
                    >
                      <span className="mc-lifecycle-name">{schedule.name}</span>
                    </li>
                  ))}
                </ul>
              )}
            </section>
            <section className="mc-lifecycle-section" aria-label={copy.effects}>
              <h3>{copy.effects}</h3>
              <ul className="mc-lifecycle-list" data-testid={`${testId}-effects`}>
                {preview.effects.map((effect) => (
                  <li key={effect} data-testid="mission-control-plan-lifecycle-effect" data-effect={effect}>
                    <span className="mc-lifecycle-name">{PLAN_PAUSE_EFFECT_LABEL[effect]}</span>
                  </li>
                ))}
              </ul>
              {preview.restore_mode && (
                <p className="mc-lifecycle-meta" data-testid={`${testId}-restore-mode`}>
                  Returns to {preview.restore_mode}
                </p>
              )}
            </section>
          </>
        )}
        {submitError && (
          <p role="alert" className="mc-lifecycle-error">
            {submitError}
          </p>
        )}
        <div className="mc-lifecycle-actions">
          <button
            type="button"
            data-autofocus
            className="mc-lifecycle-button"
            data-testid={`mission-control-plan-${kind}-cancel`}
            disabled={submitting}
            onClick={cancel}
          >
            Cancel
          </button>
          <button
            type="button"
            className="mc-lifecycle-button mc-lifecycle-button-primary"
            data-testid={`mission-control-plan-${kind}-confirm`}
            disabled={!canConfirm}
            onClick={() => void confirm()}
          >
            {copy.confirm}
          </button>
        </div>
      </div>
    </div>,
    document.body,
  );
}
