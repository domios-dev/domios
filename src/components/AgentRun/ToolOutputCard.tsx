import { useState } from "react";

export interface ToolOutputCardProps {
  kind: "call" | "result";
  toolName?: string;
  payload: string;
  isError?: boolean;
}

/**
 * Shared, transport-agnostic tool card. `payload` is deliberately never
 * shortened: the compact body is only a visual preview while expand, copy and
 * assistive technology continue to operate on this original string.
 */
export function ToolOutputCard({
  kind,
  toolName,
  payload,
  isError = false,
}: ToolOutputCardProps) {
  const [open, setOpen] = useState(false);
  const [copied, setCopied] = useState(false);
  const lineCount = payload ? payload.split(/\r?\n/).length : 0;
  // A long unbroken transport envelope also needs an affordance; relying on
  // newline count would silently clip it at narrow widths.
  const expandable = lineCount > 1 || payload.length > 140;
  const label = kind === "call" ? `${toolName ?? "tool"} call` : isError ? "Error output" : "Tool output";
  const preview = payload.replace(/\r?\n/g, " ⏎ ");

  const copy = async () => {
    try {
      await navigator.clipboard?.writeText(payload);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } catch {
      // Keep the complete value in the DOM even where Clipboard is unavailable.
      setCopied(false);
    }
  };

  return (
    <section
      className="ct-tool-output-card min-w-0 max-w-full text-[10px]"
      data-testid="tool-output-card"
      data-tool-kind={kind}
      data-tool-name={toolName}
      data-error={isError ? "true" : "false"}
      aria-label={label}
    >
      <div
        className="flex min-w-0 items-center gap-1.5"
        style={{ color: isError ? "var(--ct-red)" : "var(--ct-muted-fg)" }}
      >
        <span aria-hidden="true" className="shrink-0">{kind === "call" ? "●" : isError ? "✗" : "⎿"}</span>
        <span className="min-w-0 truncate font-medium text-[var(--ct-fg)]">{label}</span>
        {kind === "result" && (
          <span className="shrink-0" data-testid="tool-output-status">
            {isError ? "failed" : "complete"}
          </span>
        )}
        {expandable && <span className="shrink-0 opacity-70">· {lineCount} {lineCount === 1 ? "line" : "lines"}</span>}
      </div>

      {!open && (
        <div className="mt-0.5 flex min-w-0 items-start gap-1.5">
          <div
            className="ct-tool-output-preview line-clamp-2 min-w-0 flex-1 text-[var(--ct-muted-fg)]"
            data-testid="tool-output-preview"
            style={{
              display: "-webkit-box",
              WebkitBoxOrient: "vertical",
              WebkitLineClamp: 2,
              overflow: "hidden",
              overflowWrap: "anywhere",
              whiteSpace: "pre-wrap",
            }}
          >
            {preview || "(no output)"}
          </div>
          {expandable && (
            <span className="flex shrink-0 items-center gap-0.5 text-[var(--ct-accent)]" aria-hidden="true">…</span>
          )}
          {expandable && (
            <button
              type="button"
              onClick={() => setOpen(true)}
              aria-label={`Expand full ${label}`}
              aria-expanded="false"
              className="shrink-0 rounded bg-transparent px-1 py-0 text-[10px] text-[var(--ct-accent)] hover:text-[var(--ct-fg)] focus-visible:outline focus-visible:outline-1 focus-visible:outline-[var(--ct-accent)]"
            >
              Expand ▸
            </button>
          )}
        </div>
      )}

      {open && (
        <div className="mt-1 min-w-0">
          <div className="mb-1 flex items-center gap-2">
            <button
              type="button"
              onClick={copy}
              className="rounded bg-transparent px-1 py-0 text-[10px] text-[var(--ct-accent)] hover:text-[var(--ct-fg)] focus-visible:outline focus-visible:outline-1 focus-visible:outline-[var(--ct-accent)]"
            >
              {copied ? "Copied" : "Copy full output"}
            </button>
            <button
              type="button"
              onClick={() => setOpen(false)}
              aria-label={`Collapse ${label}`}
              aria-expanded="true"
              className="rounded bg-transparent px-1 py-0 text-[10px] text-[var(--ct-accent)] hover:text-[var(--ct-fg)] focus-visible:outline focus-visible:outline-1 focus-visible:outline-[var(--ct-accent)]"
            >
              Collapse ▾
            </button>
          </div>
          <pre
            className="ct-tool-output-full max-w-full overflow-auto rounded bg-[var(--ct-bg-alt,rgba(255,255,255,0.03))] p-2 text-[10px] whitespace-pre-wrap text-[var(--ct-muted-fg)]"
            data-testid="tool-output-full"
            tabIndex={0}
            style={{ maxHeight: "min(240px, 45vh)", overflowWrap: "anywhere" }}
          >
            {payload}
          </pre>
        </div>
      )}
    </section>
  );
}

/** Normalizes every transport to a display string without discarding envelope metadata. */
export function stringifyToolPayload(content: unknown): string {
  if (typeof content === "string") return content;
  if (content && typeof content === "object") return JSON.stringify(content, null, 2);
  return String(content ?? "");
}
