import { useEffect, useId, useRef, useState, type FormEvent, type KeyboardEvent, type MouseEvent } from "react";
import { createPortal } from "react-dom";
import { Button } from "./Button";
import { UI_INPUT } from "./design";
import { trapDialogTab } from "./dialogFocusTrap";

export interface TextPromptDialogProps {
  title: string;
  label: string;
  description?: string;
  placeholder?: string;
  initialValue?: string;
  confirmLabel?: string;
  validate?(value: string): string | null;
  onSubmit(value: string): void;
  onCancel(): void;
}

export function TextPromptDialog({
  title,
  label,
  description,
  placeholder,
  initialValue = "",
  confirmLabel = "OK",
  validate,
  onSubmit,
  onCancel,
}: TextPromptDialogProps) {
  const dialog = useRef<HTMLFormElement | null>(null);
  const input = useRef<HTMLInputElement | null>(null);
  const [value, setValue] = useState(initialValue);
  const titleId = useId();
  const descriptionId = useId();
  const inputId = useId();
  const errorId = useId();
  const trimmed = value.trim();
  const error = trimmed ? validate?.(trimmed) ?? null : null;
  const canSubmit = trimmed.length > 0 && error === null;

  useEffect(() => {
    input.current?.focus();
    input.current?.select();
  }, []);

  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (canSubmit) onSubmit(trimmed);
  };

  // Stops at the dialog so a host's own Escape or key forwarding never sees typing meant for the field.
  const onKeyDown = (event: KeyboardEvent<HTMLFormElement>) => {
    event.stopPropagation();
    if (event.key === "Escape") {
      event.preventDefault();
      onCancel();
      return;
    }
    trapDialogTab(event, dialog.current);
  };

  const onBackdropMouseDown = (event: MouseEvent<HTMLDivElement>) => {
    if (event.target === event.currentTarget) onCancel();
  };

  return createPortal(
    <div
      className="fixed inset-0 z-[1000] grid place-items-center bg-[color-mix(in_srgb,var(--ct-bg)_60%,transparent)] p-4"
      onMouseDown={onBackdropMouseDown}
    >
      <form
        ref={dialog}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        aria-describedby={description ? descriptionId : undefined}
        className="flex w-full max-w-sm flex-col gap-3 rounded-lg border border-[var(--ct-border)] bg-[var(--ct-surface)] p-4 text-[var(--ct-fg)] shadow-xl"
        onKeyDown={onKeyDown}
        onSubmit={submit}
      >
        <h2 id={titleId} className="text-[length:var(--ct-font-size-md)] font-semibold">{title}</h2>
        {description && <p id={descriptionId} className="text-sm text-[var(--ct-muted-fg)]">{description}</p>}
        <label htmlFor={inputId} className="flex flex-col gap-1 text-xs text-[var(--ct-muted-fg)]">
          <span>{label}</span>
          <input
            ref={input}
            id={inputId}
            type="text"
            className={UI_INPUT.full}
            placeholder={placeholder}
            autoComplete="off"
            spellCheck={false}
            value={value}
            aria-invalid={error !== null}
            aria-describedby={error ? errorId : undefined}
            onChange={(event) => setValue(event.target.value)}
          />
        </label>
        {error && <p id={errorId} role="alert" className="text-xs text-[var(--ct-red)]">{error}</p>}
        <div className="flex justify-end gap-2">
          <Button size="sm" onClick={onCancel}>Cancel</Button>
          <Button size="sm" variant="primary" type="submit" disabled={!canSubmit}>{confirmLabel}</Button>
        </div>
      </form>
    </div>,
    document.body,
  );
}
