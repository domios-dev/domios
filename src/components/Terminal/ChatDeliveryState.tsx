import type { ChatDelivery } from "./chatSendStatus";

function messageExcerpt(message: string): string {
  const compact = message.replace(/\s+/g, " ").trim();
  return compact.length > 80 ? `${compact.slice(0, 77)}…` : compact;
}

export function chatMessageAccessibleName(type: string, content: string): string {
  const excerpt = messageExcerpt(content);
  return excerpt ? `${type} message: ${excerpt}` : `${type} message`;
}

function isSubmissionUnconfirmed(error: string | null): boolean {
  return /submit_unconfirmed|not acknowledged|not confirmed|never confirmed/i.test(error ?? "");
}

export function ChatDeliveryState({
  delivery,
  onRetry,
  retrying = false,
}: {
  delivery: ChatDelivery;
  onRetry: (delivery: ChatDelivery) => void;
  retrying?: boolean;
}) {
  const excerpt = messageExcerpt(delivery.message);
  if (delivery.status.state === "pending") {
    return (
      <div
        role="status"
        aria-label={`Sending message: ${excerpt}`}
        className="mt-2 text-xs"
        style={{ color: "var(--ct-muted-fg)" }}
      >
        Sending…
      </div>
    );
  }
  if (delivery.status.state === "sent") {
    return (
      <div
        role="status"
        aria-label={`Submission confirmed for message: ${excerpt}`}
        className="mt-2 text-xs"
        style={{ color: "var(--ct-muted-fg)" }}
      >
        Submission confirmed.
      </div>
    );
  }

  const unconfirmed = isSubmissionUnconfirmed(delivery.status.error);
  const title = unconfirmed ? "Submission unconfirmed" : "Message delivery failed";
  return (
    <div
      role="alert"
      aria-label={`${title} for message: ${excerpt}`}
      className="mt-2 rounded-md px-3 py-2 text-xs"
      style={{
        background: "color-mix(in srgb, var(--ct-red, #f85149) 10%, transparent)",
        border: "1px solid color-mix(in srgb, var(--ct-red, #f85149) 38%, transparent)",
        color: "var(--ct-fg)",
      }}
    >
      <div className="font-semibold">{title}.</div>
      <div className="mt-1">
        {unconfirmed
          ? "CodeTerm could not confirm that the agent accepted this message."
          : "CodeTerm could not deliver this message."}
      </div>
      {delivery.status.error && (
        <div className="mt-1 opacity-75">{delivery.status.error}</div>
      )}
      <button
        type="button"
        aria-label={`Retry sending message: ${excerpt}`}
        onClick={() => onRetry(delivery)}
        disabled={retrying}
        className="mt-2 rounded px-2 py-1 font-medium hover:brightness-125"
        style={{
          background: "var(--ct-surface)",
          border: "1px solid var(--ct-border)",
          color: "var(--ct-fg)",
        }}
      >
        {retrying ? "Retrying…" : "Retry"}
      </button>
    </div>
  );
}
