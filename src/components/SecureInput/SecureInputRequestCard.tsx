import { useEffect, useRef, useState } from "react";
import { Lock, Send, X } from "lucide-react";

export type SecureInputKind = "code" | "token" | "password" | "key";
export type SecureInputStatus =
  | "pending"
  | "submitting"
  | "delivered"
  | "refused"
  | "expired"
  | "uncertain";

export type SecureInputConsumer =
  | {
      type: "provider_login";
      tab_id: string;
      provider_id?: string | null;
      login_id?: string | null;
      session_id?: string | null;
      nonce?: string | null;
      epoch?: number | null;
      method_id?: string | null;
    }
  | {
      type: "vault_slot";
      name: string;
      overwrite?: boolean;
    }
  | {
      type: "provider_key";
      provider_id: string;
      tab_id?: string | null;
      method_id?: string | null;
    };

export interface SecureInputRequestView {
  requestId: string;
  targetInstallationId: string;
  targetMachineName: string;
  isRemote: boolean;
  consumer: SecureInputConsumer;
  targetLabel: string;
  kind: SecureInputKind;
  reason: string;
  status: SecureInputStatus;
  reasonDetail?: string | null;
  requesterTabId?: string | null;
  createdAtMs: number;
  expiresAtMs: number;
}

interface Props {
  request: SecureInputRequestView;
  onSubmit: (
    requestId: string,
    value: string,
    expectedInstallationId: string,
  ) => Promise<SecureInputRequestView>;
  onRefuse: (requestId: string) => Promise<void>;
  onDismiss?: (requestId: string) => void;
}

const TERMINAL_OR_LOCKED_STATUSES: ReadonlySet<SecureInputStatus> = new Set([
  "delivered",
  "uncertain",
  "refused",
  "expired",
]);

function consumerSummary(request: SecureInputRequestView): string {
  switch (request.consumer.type) {
    case "vault_slot":
      return request.consumer.overwrite
        ? `Secret store slot "${request.consumer.name}" (overwrites existing)`
        : `Secret store slot "${request.consumer.name}"`;
    case "provider_login": {
      const parts = [`Provider sign-in on tab ${request.consumer.tab_id}`];
      if (request.consumer.method_id) parts.push(`method: ${request.consumer.method_id}`);
      return parts.join(" · ");
    }
    case "provider_key": {
      const parts = [`Provider API key for ${request.consumer.provider_id}`];
      if (request.consumer.tab_id) parts.push(`tab: ${request.consumer.tab_id}`);
      if (request.consumer.method_id) parts.push(`method: ${request.consumer.method_id}`);
      return parts.join(" · ");
    }
  }
}

function kindLabel(kind: SecureInputKind): string {
  switch (kind) {
    case "code":
      return "Verification code";
    case "token":
      return "Access token";
    case "password":
      return "Password";
    case "key":
      return "API key";
  }
}

function containsSecretVariant(text: string, raw: string): boolean {
  if (raw.length > 0 && text.includes(raw)) return true;
  const trimmed = raw.trim();
  return trimmed.length > 0 && text.includes(trimmed);
}

export function SecureInputRequestCard({
  request,
  onSubmit,
  onRefuse,
  onDismiss,
}: Props) {
  const [value, setValue] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [localError, setLocalError] = useState<string | null>(null);
  const [settled, setSettled] = useState<SecureInputRequestView>(request);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    // Never downgrade a locally observed terminal/uncertain status back to pending or submitting (P1-5).
    setSettled((prev) => {
      if (prev.requestId !== request.requestId) return request;
      if (
        TERMINAL_OR_LOCKED_STATUSES.has(prev.status) &&
        (request.status === "pending" || request.status === "submitting")
      ) {
        return prev;
      }
      return request;
    });
  }, [request]);

  const status = settled.status;
  const locked = submitting || status !== "pending";

  const handleSubmit = async (event: React.FormEvent) => {
    event.preventDefault();
    if (locked) return;
    const raw = value;
    if (!raw.trim()) return;
    if (inputRef.current) inputRef.current.value = "";
    setValue("");
    setSubmitting(true);
    setLocalError(null);
    setSettled((prev) => ({ ...prev, status: "submitting" }));
    try {
      const updated = await onSubmit(
        settled.requestId,
        raw,
        settled.targetInstallationId,
      );
      setSettled(updated);
    } catch (err) {
      const text = err instanceof Error ? err.message : String(err);
      const safe = containsSecretVariant(text, raw)
        ? "Submission failed. Check status before retrying."
        : text;
      setLocalError(safe);
      setSettled((prev) => ({
        ...prev,
        status: "uncertain",
        reasonDetail:
          "Submission outcome was uncertain; create a new request instead of replaying.",
      }));
    } finally {
      setSubmitting(false);
    }
  };

  const handleRefuse = async () => {
    if (submitting || status !== "pending") return;
    if (inputRef.current) inputRef.current.value = "";
    setValue("");
    setSubmitting(true);
    setLocalError(null);
    try {
      await onRefuse(settled.requestId);
    } catch (err) {
      setLocalError(err instanceof Error ? err.message : String(err));
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <section
      aria-label={`Secure input request for ${settled.targetMachineName}`}
      data-request-id={settled.requestId}
      data-status={status}
      className="rounded-xl border border-amber-500/35 bg-zinc-950/95 p-3.5 text-zinc-100 shadow-xl backdrop-blur-sm"
    >
      <div className="flex items-start justify-between gap-2">
        <div className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-amber-300">
          <Lock className="h-3.5 w-3.5 shrink-0" aria-hidden="true" />
          <span>Secure Input Request</span>
          <span className="rounded bg-amber-500/15 px-1.5 py-0.5 text-[10px] font-medium text-amber-200">
            {settled.isRemote
              ? `Remote: ${settled.targetMachineName}`
              : `Local: ${settled.targetMachineName}`}
          </span>
        </div>
        {TERMINAL_OR_LOCKED_STATUSES.has(status) && onDismiss && (
          <button
            type="button"
            aria-label="Dismiss secure input request"
            onClick={() => onDismiss(settled.requestId)}
            className="rounded p-1 text-zinc-400 hover:bg-zinc-800 hover:text-zinc-100"
          >
            <X className="h-3.5 w-3.5" aria-hidden="true" />
          </button>
        )}
      </div>

      <p className="mt-2 text-sm text-zinc-100">{settled.reason}</p>

      <dl className="mt-2 grid grid-cols-[auto_1fr] gap-x-2 gap-y-0.5 text-xs text-zinc-400">
        <dt className="font-medium text-zinc-300">Destination:</dt>
        <dd className="truncate">{consumerSummary(settled)}</dd>
        <dt className="font-medium text-zinc-300">Kind:</dt>
        <dd>{kindLabel(settled.kind)}</dd>
      </dl>

      {status === "pending" && (
        <form onSubmit={handleSubmit} className="mt-3 flex flex-col gap-2">
          <input
            ref={inputRef}
            type="password"
            value={value}
            onChange={(e) => setValue(e.target.value)}
            placeholder={`Enter ${kindLabel(settled.kind).toLowerCase()}…`}
            aria-label="Secret value"
            autoComplete="off"
            spellCheck={false}
            disabled={locked}
            className="w-full rounded-md border border-zinc-700 bg-zinc-900 px-2.5 py-1.5 text-sm text-zinc-100 placeholder-zinc-500 focus:border-amber-400 focus:outline-none"
          />
          <div className="flex items-center justify-between gap-2">
            <span className="text-[11px] text-zinc-400">
              Sent directly to the target consumer; never shown to agents or logged.
            </span>
            <div className="flex items-center gap-1.5">
              <button
                type="button"
                onClick={handleRefuse}
                disabled={locked}
                className="rounded-md border border-zinc-700 px-2.5 py-1 text-xs font-medium text-zinc-300 hover:bg-zinc-800 disabled:opacity-50"
              >
                Refuse
              </button>
              <button
                type="submit"
                disabled={locked || !value.trim()}
                className="inline-flex items-center gap-1 rounded-md bg-amber-500 px-2.5 py-1 text-xs font-semibold text-zinc-950 hover:bg-amber-400 disabled:opacity-50"
              >
                <Send className="h-3 w-3" aria-hidden="true" />
                Send securely
              </button>
            </div>
          </div>
        </form>
      )}

      {status === "submitting" && (
        <p role="status" className="mt-2.5 text-xs font-medium text-amber-200">
          Sending securely to {settled.targetLabel}…
        </p>
      )}
      {status === "delivered" && (
        <p role="status" className="mt-2.5 text-xs font-medium text-emerald-400">
          Delivered directly to {settled.targetLabel}.
        </p>
      )}
      {status === "uncertain" && (
        <p role="status" className="mt-2.5 text-xs font-medium text-amber-300">
          {settled.reasonDetail ||
            "Delivery outcome was uncertain. Do not replay; create a new request if needed."}
        </p>
      )}
      {status === "refused" && (
        <p role="status" className="mt-2.5 text-xs font-medium text-zinc-400">
          {settled.reasonDetail || "Request refused."}
        </p>
      )}
      {status === "expired" && (
        <p role="status" className="mt-2.5 text-xs font-medium text-zinc-400">
          Request expired before submission.
        </p>
      )}
      {localError && (
        <p role="alert" className="mt-1.5 text-xs text-rose-400">
          {localError}
        </p>
      )}
    </section>
  );
}
