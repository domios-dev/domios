
import React from "react";

export interface FormRowProps extends React.HTMLAttributes<HTMLDivElement> {
  label: string;
  htmlFor?: string;
  hint?: string;
  error?: string;
  children: React.ReactNode;
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
  ...rest
}: FormRowProps) {
  return (
    <div
      {...rest}
      className={`flex flex-col gap-1 ${className ?? ""}`}
      data-testid={testId}
    >
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
