import { useEffect, useMemo, useState } from "react";
import { FileDiffPreview } from "@codeterm/chat-ui/FileDiffCards";
import { ExternalLink } from "lucide-react";
import { invoke } from "../../lib/bridge";
import type { PendingPrompt, PromptElicitation, PromptElicitationField } from "../../lib/types";
import { Button } from "../shared/Button";
import { useApprovalAnswer } from "./useApprovalAnswer";
import { approvalActions, permissionHeaderTitle } from "./composerModels";

function shortFileName(path: string): string {
  const parts = path.replace(/\\/g, "/").split("/").filter(Boolean);
  return parts.length > 1 ? parts.slice(-2).join("/") : path;
}

function initialFormValues(fields: readonly PromptElicitationField[]): Record<string, string | boolean> {
  const values: Record<string, string | boolean> = {};
  for (const field of fields) {
    if (field.type === "boolean") {
      values[field.name] = typeof field.default === "boolean" ? field.default : false;
    } else if (field.default !== undefined && field.default !== null) {
      values[field.name] = String(field.default);
    } else if (field.type === "string" && field.enum?.length) {
      values[field.name] = field.enum[0];
    } else {
      values[field.name] = "";
    }
  }
  return values;
}

function validateFormValues(
  fields: readonly PromptElicitationField[],
  values: Record<string, string | boolean>,
): { ok: true; content: Record<string, unknown> } | { ok: false; error: string } {
  const content: Record<string, unknown> = {};
  for (const field of fields) {
    const label = field.title || field.name;
    const raw = values[field.name];
    if (field.type === "boolean") {
      content[field.name] = Boolean(raw);
      continue;
    }
    const text = typeof raw === "string" ? raw : "";
    if (!text.trim()) {
      if (field.required) return { ok: false, error: `${label} is required.` };
      continue;
    }
    if (field.type === "string") {
      if (field.minLength !== undefined && text.length < field.minLength) {
        return { ok: false, error: `${label} must be at least ${field.minLength} characters.` };
      }
      if (field.maxLength !== undefined && text.length > field.maxLength) {
        return { ok: false, error: `${label} must be at most ${field.maxLength} characters.` };
      }
      if (field.enum && !field.enum.includes(text)) {
        return { ok: false, error: `${label} must match an allowed option.` };
      }
      content[field.name] = text;
    } else {
      const parsed = Number(text);
      if (!Number.isFinite(parsed)) return { ok: false, error: `${label} must be a valid number.` };
      if (field.type === "integer" && !Number.isInteger(parsed)) {
        return { ok: false, error: `${label} must be a whole number.` };
      }
      if (field.minimum !== undefined && parsed < field.minimum) {
        return { ok: false, error: `${label} must be at least ${field.minimum}.` };
      }
      if (field.maximum !== undefined && parsed > field.maximum) {
        return { ok: false, error: `${label} must be at most ${field.maximum}.` };
      }
      content[field.name] = parsed;
    }
  }
  return { ok: true, content };
}

function FormElicitationDock({
  elicitation,
  tabId,
  busy,
  onAnswer,
}: {
  elicitation: Extract<PromptElicitation, { mode: "form" }>;
  tabId: string;
  busy: boolean;
  onAnswer: (value: string) => Promise<void>;
}) {
  const [values, setValues] = useState<Record<string, string | boolean>>(() => initialFormValues(elicitation.fields));
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setValues(initialFormValues(elicitation.fields));
    setError(null);
  }, [elicitation]);

  const handleSubmit = (event: React.FormEvent) => {
    event.preventDefault();
    const checked = validateFormValues(elicitation.fields, values);
    if (!checked.ok) {
      setError(checked.error);
      return;
    }
    setError(null);
    void onAnswer(JSON.stringify({ action: "accept", content: checked.content }));
  };

  return (
    <section
      aria-label="Approval dock"
      data-approval-tab={tabId}
      data-dock-mode="elicitation-form"
      className="mb-2 rounded-lg border px-3.5 py-3"
      style={{ borderColor: "var(--ct-border)", background: "var(--ct-surface)", color: "var(--ct-fg)" }}
    >
      <div className="mb-2.5 flex items-center gap-2 text-xs font-medium">
        <span aria-hidden="true" className="h-2 w-2 shrink-0 rounded-full" style={{ background: "var(--ct-accent)" }} />
        <span className="min-w-0 truncate">{elicitation.message}</span>
      </div>
      <form onSubmit={handleSubmit} className="flex flex-col gap-2.5">
        {elicitation.fields.map((field) => {
          const inputId = `elicitation-${tabId}-${field.name}`;
          const label = field.title || field.name;
          const value = values[field.name];
          if (field.type === "boolean") {
            return (
              <div key={field.name} className="flex flex-col gap-1">
                <label htmlFor={inputId} className="inline-flex cursor-pointer items-center gap-2 text-xs" style={{ color: "var(--ct-fg)" }}>
                  <input
                    id={inputId}
                    type="checkbox"
                    disabled={busy}
                    checked={Boolean(value)}
                    onChange={(event) => setValues((prev) => ({ ...prev, [field.name]: event.target.checked }))}
                    className="h-3.5 w-3.5 rounded border"
                    style={{ accentColor: "var(--ct-accent)", borderColor: "var(--ct-border)" }}
                  />
                  <span>{label}</span>
                </label>
                {field.description && <p className="text-[11px]" style={{ color: "var(--ct-muted-fg)" }}>{field.description}</p>}
              </div>
            );
          }
          return (
            <div key={field.name} className="flex flex-col gap-1">
              <label htmlFor={inputId} className="text-[11px] font-medium" style={{ color: "var(--ct-fg)" }}>
                {label}{field.required ? <span style={{ color: "var(--ct-red)" }}> *</span> : null}
              </label>
              {field.type === "string" && field.enum?.length ? (
                <select
                  id={inputId}
                  disabled={busy}
                  value={typeof value === "string" ? value : ""}
                  onChange={(event) => setValues((prev) => ({ ...prev, [field.name]: event.target.value }))}
                  className="rounded-md border px-2.5 py-1.5 text-xs outline-none focus-visible:border-[var(--ct-accent)]"
                  style={{ borderColor: "var(--ct-border)", background: "var(--ct-bg)", color: "var(--ct-fg)" }}
                >
                  {!field.required && <option value="">Select…</option>}
                  {field.enum.map((option, index) => (
                    <option key={option} value={option}>{field.enumNames?.[index] ?? option}</option>
                  ))}
                </select>
              ) : (
                <input
                  id={inputId}
                  disabled={busy}
                  type={field.type === "number" || field.type === "integer"
                    ? "number"
                    : field.format === "password"
                      ? "password"
                      : field.format === "email"
                        ? "email"
                        : field.format === "date"
                          ? "date"
                          : "text"}
                  step={field.type === "integer" ? 1 : field.type === "number" ? "any" : undefined}
                  min={field.minimum}
                  max={field.maximum}
                  minLength={field.minLength}
                  maxLength={field.maxLength}
                  value={typeof value === "string" ? value : ""}
                  onChange={(event) => setValues((prev) => ({ ...prev, [field.name]: event.target.value }))}
                  className="rounded-md border px-2.5 py-1.5 text-xs outline-none focus-visible:border-[var(--ct-accent)]"
                  style={{ borderColor: "var(--ct-border)", background: "var(--ct-bg)", color: "var(--ct-fg)" }}
                />
              )}
              {field.description && <p className="text-[11px]" style={{ color: "var(--ct-muted-fg)" }}>{field.description}</p>}
            </div>
          );
        })}
        {error && (
          <p role="alert" className="text-xs" style={{ color: "var(--ct-red)" }}>
            {error}
          </p>
        )}
        <div className="flex flex-wrap items-center gap-2 pt-0.5">
          <Button type="submit" variant="primary" size="sm" disabled={busy}>Submit</Button>
          <Button type="button" variant="ghost" size="sm" disabled={busy} onClick={() => void onAnswer(JSON.stringify({ action: "decline" }))}>Decline</Button>
          <Button type="button" variant="danger" size="sm" disabled={busy} onClick={() => void onAnswer(JSON.stringify({ action: "cancel" }))}>Cancel</Button>
        </div>
      </form>
    </section>
  );
}

function UrlElicitationDock({
  elicitation,
  tabId,
  busy,
  onAnswer,
}: {
  elicitation: Extract<PromptElicitation, { mode: "url" }>;
  tabId: string;
  busy: boolean;
  onAnswer: (value: string) => Promise<void>;
}) {
  const [openError, setOpenError] = useState<string | null>(null);

  const handleOpen = async () => {
    setOpenError(null);
    try {
      await invoke("open_url", { url: elicitation.url });
      await onAnswer("accept");
    } catch (error) {
      setOpenError(error instanceof Error ? error.message : String(error));
    }
  };

  return (
    <section
      aria-label="Approval dock"
      data-approval-tab={tabId}
      data-dock-mode="elicitation-url"
      className="mb-2 rounded-lg border px-3.5 py-3"
      style={{ borderColor: "var(--ct-border)", background: "var(--ct-surface)", color: "var(--ct-fg)" }}
    >
      <div className="flex items-center gap-2 text-xs font-medium">
        <span aria-hidden="true" className="h-2 w-2 shrink-0 rounded-full" style={{ background: "var(--ct-accent)" }} />
        <span className="min-w-0 truncate">{elicitation.message}</span>
      </div>
      <div className="my-2 flex items-center gap-2 text-xs" style={{ color: "var(--ct-muted-fg)" }}>
        <span>Target host</span>
        <code
          data-testid="elicitation-url-host"
          className="rounded px-1.5 py-0.5 font-mono text-[11px]"
          style={{ background: "var(--ct-bg)", color: "var(--ct-fg)" }}
        >
          {elicitation.host}
        </code>
      </div>
      {openError && (
        <p role="alert" className="mb-2 text-xs" style={{ color: "var(--ct-red)" }}>
          {openError}
        </p>
      )}
      <div className="flex flex-wrap items-center gap-2">
        <Button type="button" variant="primary" size="sm" disabled={busy} onClick={() => void handleOpen()}>
          <ExternalLink aria-hidden="true" size={12} />
          <span>Open sign-in</span>
        </Button>
        <Button type="button" variant="ghost" size="sm" disabled={busy} onClick={() => void onAnswer("decline")}>
          Decline
        </Button>
      </div>
    </section>
  );
}

export function ComposerApprovalDock({ prompt, tabId, active, onRespond }: { prompt: PendingPrompt; tabId: string; active: boolean; onRespond: (value: string) => Promise<unknown> }) {
  const { answer, busy } = useApprovalAnswer(prompt, tabId, active, onRespond);
  const [overflow, setOverflow] = useState(false);

  const actions = useMemo(() => approvalActions(prompt), [prompt]);
  const primary = actions.filter((option) => option.rank <= 2);
  const extra = actions.filter((option) => option.rank > 2);

  if (prompt.elicitation?.mode === "form") {
    return <FormElicitationDock elicitation={prompt.elicitation} tabId={tabId} busy={busy} onAnswer={answer} />;
  }
  if (prompt.elicitation?.mode === "url") {
    return <UrlElicitationDock elicitation={prompt.elicitation} tabId={tabId} busy={busy} onAnswer={answer} />;
  }

  const diffs = prompt.file_diffs?.length ? prompt.file_diffs : undefined;
  const headerTitle = permissionHeaderTitle(prompt);
  const totalAdditions = diffs ? diffs.reduce((sum, item) => sum + item.additions, 0) : 0;
  const totalDeletions = diffs ? diffs.reduce((sum, item) => sum + item.deletions, 0) : 0;

  return (
    <section
      aria-label="Approval dock"
      data-approval-tab={tabId}
      data-dock-mode={diffs ? "permission-diff" : "permission"}
      className="mb-2 rounded-lg border px-3.5 py-3"
      style={{ borderColor: "var(--ct-border)", background: "var(--ct-surface)", color: "var(--ct-fg)" }}
    >
      <div className="flex min-w-0 items-center justify-between gap-2 text-xs">
        <div className="flex min-w-0 items-center gap-2">
          <span aria-hidden="true" className="h-2 w-2 shrink-0 rounded-full" style={{ background: "var(--ct-yellow, var(--ct-accent))" }} />
          <span data-testid="approval-dock-title" className="truncate font-medium" style={{ color: "var(--ct-fg)" }}>
            {headerTitle}
          </span>
          {diffs && (
            <span className="min-w-0 truncate font-mono text-[11px]" style={{ color: "var(--ct-muted-fg)" }}>
              {diffs.length === 1 ? shortFileName(diffs[0].path) : `${diffs.length} files`}
            </span>
          )}
        </div>
        {diffs && (
          <span data-testid="approval-dock-diff-stats" className="shrink-0 font-mono text-[11px]">
            <span style={{ color: "var(--ct-green)" }}>+{totalAdditions}</span>{" "}
            <span style={{ color: "var(--ct-red)" }}>-{totalDeletions}</span>
          </span>
        )}
      </div>

      {diffs ? (
        <div className="my-2.5 flex flex-col gap-2">
          {diffs.map((item) => (
            <div key={item.path} className="flex flex-col gap-1">
              {diffs.length > 1 && (
                <div className="flex items-center justify-between gap-2 font-mono text-[11px]" style={{ color: "var(--ct-muted-fg)" }}>
                  <span className="truncate">{shortFileName(item.path)}</span>
                  <span className="shrink-0">
                    <span style={{ color: "var(--ct-green)" }}>+{item.additions}</span>{" "}
                    <span style={{ color: "var(--ct-red)" }}>-{item.deletions}</span>
                  </span>
                </div>
              )}
              <FileDiffPreview diff={item} maxHeight="10rem" />
            </div>
          ))}
        </div>
      ) : (
        <pre className="my-2 max-h-32 overflow-auto whitespace-pre-wrap break-words font-mono text-xs" style={{ color: "var(--ct-muted-fg)" }}>
          {prompt.question}
        </pre>
      )}

      <div className="flex flex-wrap items-center gap-2">
        {primary.map((option) => (
          <Button
            key={option.value}
            type="button"
            variant={option.tone}
            size="sm"
            disabled={busy}
            onClick={() => void answer(option.value)}
          >
            {option.label}
          </Button>
        ))}
        {extra.length > 0 && (
          <Button
            type="button"
            variant="ghost"
            size="sm"
            aria-label="More approval options"
            aria-expanded={overflow}
            disabled={busy}
            onClick={() => setOverflow((value) => !value)}
          >
            …
          </Button>
        )}
        {overflow && extra.map((option) => (
          <Button
            key={option.value}
            type="button"
            variant={option.tone}
            size="sm"
            disabled={busy}
            onClick={() => void answer(option.value)}
          >
            {option.label}
          </Button>
        ))}
      </div>
    </section>
  );
}
