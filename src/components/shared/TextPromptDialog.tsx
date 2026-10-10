import { useEffect, useId, useRef, useState, type FormEvent } from "react";
import { Button } from "./Button";
import { UI_INPUT } from "./design";
import { FormDialog } from "./FormDialog";

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

  return (
      <FormDialog titleId={titleId} descriptionId={description ? descriptionId : undefined} onCancel={onCancel} onSubmit={submit}>
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
      </FormDialog>
  );
}
