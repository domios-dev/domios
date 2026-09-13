import { useEffect, useId, useRef, useState, type FormEvent, type KeyboardEvent, type MouseEvent } from "react";
import { createPortal } from "react-dom";
import { trapDialogTab } from "./dialogFocusTrap";

export interface MissionControlActionRequest {
  key: string;
  title: string;
  message: string;
  confirmLabel: string;
  destructive?: boolean;
  /** Present ⇒ the action takes a required, non-blank reason. */
  reasonLabel?: string;
  run: (reason: string) => void | Promise<void>;
}

export interface MissionControlActionDialogProps {
  request: MissionControlActionRequest;
  onClose: () => void;
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export function MissionControlActionDialog({ request, onClose }: MissionControlActionDialogProps) {
  const dialogRef = useRef<HTMLFormElement | null>(null);
  const [reason, setReason] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const titleId = useId();
  const messageId = useId();
  const reasonId = useId();
  const testId = `mission-control-${request.key}-dialog`;
  const needsReason = request.reasonLabel !== undefined;
  const trimmedReason = reason.trim();
  const canConfirm = !submitting && (!needsReason || trimmedReason.length > 0);

  useEffect(() => {
    dialogRef.current?.querySelector<HTMLElement>("[data-autofocus]")?.focus();
  }, []);

  const cancel = () => {
    if (!submitting) onClose();
  };

  const submit = async (event?: FormEvent) => {
    event?.preventDefault();
    if (!canConfirm) return;
    setSubmitting(true);
    setError(null);
    try {
      await request.run(trimmedReason);
      onClose();
    } catch (failure) {
      setError(errorMessage(failure));
      setSubmitting(false);
    }
  };

  const onKeyDown = (event: KeyboardEvent<HTMLFormElement>) => {
    event.stopPropagation();
    if (event.key === "Escape") {
      event.preventDefault();
      cancel();
      return;
    }
    trapDialogTab(event, dialogRef.current);
  };

  const onBackdropMouseDown = (event: MouseEvent<HTMLDivElement>) => {
    if (event.target === event.currentTarget) cancel();
  };

  return createPortal(
    <div className="mc-lifecycle-backdrop" data-testid={`${testId}-backdrop`} onMouseDown={onBackdropMouseDown}>
      <form
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        aria-describedby={messageId}
        aria-busy={submitting}
        data-testid={testId}
        className="mc-lifecycle-dialog"
        onKeyDown={onKeyDown}
        onSubmit={(event) => void submit(event)}
        onClick={(event) => event.stopPropagation()}
      >
        <h2 id={titleId} className="mc-lifecycle-title">
          {request.title}
        </h2>
        <p id={messageId} className="mc-lifecycle-meta">
          {request.message}
        </p>
        {needsReason && (
          <label className="mc-action-field" htmlFor={reasonId}>
            <span>{request.reasonLabel}</span>
            <input
              id={reasonId}
              type="text"
              data-autofocus
              data-testid={`${testId}-reason`}
              className="mc-action-input"
              autoComplete="off"
              required
              aria-required="true"
              disabled={submitting}
              value={reason}
              onChange={(event) => setReason(event.target.value)}
            />
          </label>
        )}
        {error && (
          <p role="alert" className="mc-lifecycle-error" data-testid={`${testId}-error`}>
            {error}
          </p>
        )}
        <div className="mc-lifecycle-actions">
          <button
            type="button"
            data-autofocus
            className="mc-lifecycle-button"
            data-testid={`${testId}-cancel`}
            disabled={submitting}
            onClick={cancel}
          >
            Cancel
          </button>
          <button
            type="button"
            className={`mc-lifecycle-button ${request.destructive ? "mc-lifecycle-button-danger" : "mc-lifecycle-button-primary"}`}
            data-testid={`${testId}-confirm`}
            disabled={!canConfirm}
            onClick={() => void submit()}
          >
            {request.confirmLabel}
          </button>
        </div>
      </form>
    </div>,
    document.body,
  );
}
