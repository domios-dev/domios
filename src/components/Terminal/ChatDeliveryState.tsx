import type { DeliveryState } from "./chatDeliveryModel";

const LABEL: Record<Exclude<DeliveryState, "delivered">, string> = {
  sending: "Sending",
  written: "Written to the agent — waiting for transcript confirmation; a busy agent will read after its current step",
  queued: "Queued for the agent",
  uncertain: "Submission unconfirmed",
  failed: "Not delivered",
};
const QUEUED_BEHIND_TURN = "Queued until the current turn ends";
const RESEND_WITH_CLI = "Not delivered. Resend with domios send";

/** `inline` sits inside a card header and shows its label; beside a bubble it stays one quiet dot, a held send adding a short "Queued". */
export function ChatDeliveryState({ state, error, onRetry, resendWithCli = false, queuedBehindTurn = false, inline = false }: {
  state: DeliveryState; error?: string | null; onRetry?: () => void; resendWithCli?: boolean; queuedBehindTurn?: boolean; inline?: boolean;
}) {
  if (state === "delivered") return null;
  const refusalMarker = state === "failed" ? "runtime_rejected_before_write:"
    : state === "uncertain" ? "agent_refused_while_busy:" : null;
  const refusal = refusalMarker && error?.includes(refusalMarker)
    ? error.slice(error.indexOf(refusalMarker) + refusalMarker.length).trim() : null;
  const resendHint = state === "failed" && resendWithCli;
  const label = refusal || (resendHint ? RESEND_WITH_CLI : state === "queued" && queuedBehindTurn ? QUEUED_BEHIND_TURN : LABEL[state]);
  const short = state === "queued" && !inline && !refusal;
  return <span role="status" aria-live="polite" aria-label={short ? label : undefined}
    className={inline ? "ct-delivery ct-delivery--inline" : "ct-delivery"} data-state={state} title={label}>
    <span className="ct-delivery__dot" aria-hidden="true" />
    {short && <span className="ct-delivery__label ct-delivery__label--short" aria-hidden="true">Queued</span>}
    <span className={refusal || inline || resendHint ? "ct-delivery__label" : "sr-only"}>{label}</span>
    {state === "failed" && onRetry && <button type="button" className="ct-delivery__retry" onClick={onRetry}>Retry</button>}
  </span>;
}
