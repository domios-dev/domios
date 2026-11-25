import type { ProviderErrorRemediation } from "@codeterm/chat-core";
import { Button } from "./Button";
import { CodeBlock } from "./CodeBlock";

export function ProviderErrorNotice({
  remediation,
  onRetry,
}: {
  remediation: ProviderErrorRemediation;
  onRetry?: () => void;
}) {
  return (
    <section
      role="alert"
      aria-label="Provider API error"
      data-testid="provider-error-notice"
      data-error-code={remediation.code}
      className="mb-2 flex w-full min-w-0 flex-col gap-2.5 rounded-lg border p-3.5 text-left text-sm"
      style={{
        background: "var(--ct-surface)",
        borderColor: "color-mix(in srgb, var(--ct-red) 45%, var(--ct-border))",
      }}
    >
      <div className="flex flex-col gap-1">
        <p className="m-0 font-medium" style={{ color: "var(--ct-red)" }}>
          {remediation.title}
        </p>
        <p className="m-0 text-xs leading-relaxed" style={{ color: "var(--ct-fg)" }}>
          {remediation.message}
        </p>
      </div>
      {remediation.commands && remediation.commands.length > 0 && (
        <div className="flex w-full min-w-0 flex-col gap-2">
          {remediation.commands.map((entry) => (
            <div key={entry.command} className="flex w-full min-w-0 flex-col gap-1">
              {entry.label && (
                <span className="text-xs" style={{ color: "var(--ct-muted-fg)" }}>
                  {entry.label}
                </span>
              )}
              <div role="region" aria-label={entry.label ? `Command: ${entry.label}` : "Command"} className="w-full min-w-0">
                <CodeBlock text={entry.command} language="bash" wrapLines />
              </div>
            </div>
          ))}
        </div>
      )}
      {remediation.retry && onRetry && (
        <div className="flex flex-wrap items-center gap-2">
          <Button variant="primary" size="sm" data-testid="provider-error-retry" onClick={onRetry}>
            Retry
          </Button>
        </div>
      )}
      {remediation.rawDetail && (
        <details className="w-full min-w-0 text-xs" style={{ color: "var(--ct-muted-fg)" }}>
          <summary className="cursor-pointer select-none">Details</summary>
          <pre className="mt-1 max-h-48 select-text overflow-auto whitespace-pre-wrap break-words font-mono text-[11px] [overflow-wrap:anywhere]">
            {remediation.rawDetail}
          </pre>
        </details>
      )}
    </section>
  );
}
