import { Lock, X } from "lucide-react";
import { AccountAvatar } from "../Titlebar/AccountAvatar";
import { accountInitials } from "../Settings/accountModel";
import { Button } from "../shared/Button";
import { neighborIdentity, type NeighborRequest } from "./neighborModel";
import { conciergeNeedsAttention, conciergeStatusLine, missingConciergeLine, type ConciergeStatus } from "./useConciergeStatus";
import { STATUS_LABEL, type NeighborNodeStatus } from "./neighborhoodModel";

export function ConciergeRecoveryNotice({ status, onSetUpWithGeneral, onOpenConcierge, onReviewAccess }: {
  status: ConciergeStatus; onSetUpWithGeneral: () => void; onOpenConcierge: (tabId: string) => void; onReviewAccess: () => void;
}) {
  if (!conciergeNeedsAttention(status)) return null;
  const setup = status.state === "concierge_closed" || status.state === "concierge_missing" || !status.gateway_tab;
  const reviewAccess = status.state === "concierge_access_needed";
  const action = reviewAccess ? "Review access" : setup ? "Set up with General" : status.state === "concierge_auth_needed" ? "Open sign-in"
    : status.state === "concierge_provider_refused" ? "Review provider" : "Open recovery";
  return <div role="status" aria-label="Concierge needs attention" className="ct-card ct-card--static neighbor-missing">
    <div className="min-w-0 flex-1">
      <p className="ct-card__title">{status.state === "concierge_missing" ? missingConciergeLine(status) : conciergeStatusLine(status)}</p>
      <p className="ct-card__meta">{reviewAccess ? "Review this friend's permissions to continue. Messages wait here."
        : setup ? "Messages wait until you choose to set up a concierge here."
        : "Open this concierge to continue with its sign-in or recovery options. Messages are not resent."}</p>
    </div>
    <div className="neighbor-confirm__actions">
      <Button size="sm" variant="primary" onClick={() => reviewAccess ? onReviewAccess() : setup ? onSetUpWithGeneral() : onOpenConcierge(status.gateway_tab!)}>{action}</Button>
    </div>
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
