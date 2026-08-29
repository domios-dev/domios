import { useState, type FormEvent } from "react";
import { rebindTabSession } from "./sessionBindingApi";

export function SessionBindingRecoveryForm({
  tabId,
  suggestedSessionId,
  onClose,
}: {
  tabId: string;
  suggestedSessionId: string | null;
  onClose: () => void;
}) {
  const [sessionId, setSessionId] = useState(suggestedSessionId ?? "");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    const requested = sessionId.trim();
    if (!requested || submitting) return;
    setSubmitting(true);
    setError(null);
    try {
      await rebindTabSession(tabId, requested);
      onClose();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
      setSubmitting(false);
    }
  };

  return (
    <form
      onSubmit={submit}
      role="group"
      aria-label="Repair session binding"
      className="mx-auto mt-2 flex w-full max-w-4xl flex-wrap items-end gap-2 px-2"
      data-testid="session-binding-recovery"
    >
      <label className="min-w-48 flex-1 text-xs text-[var(--ct-muted-fg)]">
        Provider session ID
        <input
          value={sessionId}
          onChange={(event) => setSessionId(event.currentTarget.value)}
          aria-invalid={error ? true : undefined}
          className="mt-1 w-full rounded-md border border-[var(--ct-border)] bg-[var(--ct-surface)] px-2 py-1 text-sm text-[var(--ct-fg)]"
        />
      </label>
      <button type="submit" disabled={!sessionId.trim() || submitting} className="rounded-md px-2 py-1 text-sm">
        {submitting ? "Rebinding…" : "Rebind"}
      </button>
      <button type="button" onClick={onClose} className="rounded-md px-2 py-1 text-sm">
        Cancel
      </button>
      {error && <span role="alert" className="w-full text-xs text-[var(--ct-danger)]">{error}</span>}
    </form>
  );
}
