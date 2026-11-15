import { Lock, X } from "lucide-react";
import { AccountAvatar } from "../Titlebar/AccountAvatar";
import { accountInitials } from "../Settings/accountModel";
import { Button } from "../shared/Button";
import { neighborIdentity, type NeighborRequest } from "./neighborModel";
import { conciergeNeedsAttention, missingConciergeLine, type ConciergeStatus } from "./useConciergeStatus";
import { STATUS_LABEL, type NeighborNodeStatus } from "./neighborhoodModel";

const RECOVERY_SENTENCES: Partial<Record<ConciergeStatus["state"], string>> = {
  concierge_auth_needed: "Sign in to continue with this concierge.",
  concierge_access_needed: "This concierge can't continue until you review its access.",
  concierge_provider_refused: "This concierge's provider is unavailable.",
  concierge_crashed: "This concierge has stopped.",
};

export function ConciergeRecoveryNotice({ status, friendName, onSetUpWithGeneral, onOpenConcierge, onReviewAccess }: {
  status: ConciergeStatus; friendName: string; onSetUpWithGeneral: () => void; onOpenConcierge: (tabId: string) => void; onReviewAccess: () => void;
}) {
  if (!conciergeNeedsAttention(status)) return null;
  const setup = status.state === "concierge_closed" || status.state === "concierge_missing" || status.state === "concierge_invalid" || !status.gateway_tab;
  const reviewAccess = status.state === "concierge_access_needed";
  const invalid = status.state === "concierge_invalid";
  const closed = status.state === "concierge_closed";
  const action = invalid || closed ? "Ask General to fix" : reviewAccess ? "Review access" : setup ? "Set up with General" : status.state === "concierge_auth_needed" ? "Open sign-in"
    : status.state === "concierge_provider_refused" ? "Review provider" : "Open recovery";
  return <div role="status" aria-label="Concierge needs attention" className="ct-card ct-card--static neighbor-missing">
    <div className="min-w-0 flex-1">
      <p className="ct-card__title">{closed ? `This concierge's tab was closed, so it can't answer ${friendName}.` : invalid ? `This concierge's tab now has another job, so it can't answer ${friendName}.` : RECOVERY_SENTENCES[status.state] ?? `${missingConciergeLine(status)}.`}</p>
    </div>
    <div>
      <Button size="sm" variant="ghost" className="text-xs text-[var(--ct-accent)]" onClick={() => reviewAccess ? onReviewAccess() : setup ? onSetUpWithGeneral() : onOpenConcierge(status.gateway_tab!)}>{action}</Button>
    </div>
      <details className="ct-card__meta">
        <summary>Details</summary>
        <p>{reviewAccess ? "Domios can't confirm what this concierge is allowed to do. Check its saved permissions before it starts again." : setup ? "Messages stay pending until you set up a concierge here." : "Messages stay pending while you review this concierge."}</p>
      </details>
  </div>;
}

/** What the relay keeps: ciphertext only until delivery, never the history. */
export function NeighborPrivacyNote({ className = "" }: { className?: string }) {
  return <p className={`neighbor-privacy ${className}`.trim()}>
    <Lock size={11} aria-hidden="true" />
    <span>End-to-end encrypted. Messages are deleted from our servers once delivered; your history lives only on your machines.</span>
  </p>;
}

function PanelHead({ name, photoURL, status, meta, onClose }: { name: string; photoURL: string | null; status: NeighborNodeStatus; meta?: string; onClose: () => void }) {
  return <header className="neighbor-panel__head">
    <span className="neighbor-panel__avatar"><AccountAvatar url={photoURL} initial={accountInitials(name)} label="" size="node" /></span>
    <div className="min-w-0 flex-1">
      <h2 className="neighbor-panel__name">{name}</h2>
      <p className="neighbor-status"><span className="neighbor-dot" data-tone={status === "connected" ? "ok" : status === "request" ? "progress" : "muted"} aria-hidden="true" />{STATUS_LABEL[status]}{meta ? <span className="neighbor-note"> · {meta}</span> : null}</p>
    </div>
    <button type="button" className="neighbor-panel__close ct-liquid-interactive" aria-label={`Close ${name}`} onClick={onClose}><X size={14} aria-hidden="true" /></button>
  </header>;
}

export function NeighborRequestPanel({ request, pending, onAccept, onDecline, onClose }: {
  request: NeighborRequest & { name: string }; pending: boolean; onAccept: () => void; onDecline: () => void; onClose: () => void;
}) {
  return <aside className="neighbor-panel" aria-label={request.name}>
    <PanelHead name={request.name} photoURL={request.photoURL} status="request" onClose={onClose} />
    <p className="neighbor-panel__lead">{neighborIdentity(request)} wants to be your Neighbor. Their agents will be able to message yours. Your General Agent will ask what they may do once you accept.</p>
    <div className="neighbor-confirm__actions">
      <Button size="sm" variant="primary" disabled={pending} onClick={onAccept} aria-label={`Accept ${request.name}`}>Accept</Button>
      <Button size="sm" disabled={pending} onClick={onDecline} aria-label={`Decline ${request.name}`}>Decline</Button>
    </div>
  </aside>;
}
