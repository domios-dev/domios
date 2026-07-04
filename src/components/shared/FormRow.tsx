
import React from "react";

export interface FormRowProps {
  label: string;
  htmlFor?: string;
  hint?: string;
  error?: string;
  children: React.ReactNode;
  className?: string;
  "data-testid"?: string;
}

export function FormRow({
  label,
  htmlFor,
  hint,
  error,
  children,
  className,
  "data-testid": testId,
}: FormRowProps) {
  return (
    <div className={`flex flex-col gap-1 ${className ?? ""}`} data-testid={testId}>
      <label
        htmlFor={htmlFor}
        className="text-[11px] font-medium text-[var(--ct-muted-fg)] uppercase tracking-wide select-none"
      >
        {label}
      </label>
      {children}
      {hint && !error ? (
        <span className="text-[10px] text-[var(--ct-muted)]">{hint}</span>
      ) : null}
      {error ? (
        <span className="text-[10px] text-[var(--ct-red)]" role="alert">
          {error}
        </span>
      ) : null}
    </div>
  );
}
