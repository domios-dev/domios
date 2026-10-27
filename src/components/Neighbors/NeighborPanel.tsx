import { Lock, X } from "lucide-react";
import { AccountAvatar } from "../Titlebar/AccountAvatar";
import { accountInitials } from "../Settings/accountModel";
import { Button } from "../shared/Button";
import type { NeighborRequest } from "./neighborModel";
import { missingConciergeLine, type ConciergeStatus } from "./useConciergeStatus";
import { STATUS_LABEL, type NeighborNodeStatus } from "./neighborhoodModel";

/** A friend whose messages wait because this machine has no concierge for them yet. */
export function MissingConciergeNotice({ status, onSetUpWithGeneral, onEditPermissions }: {
  status: ConciergeStatus; onSetUpWithGeneral: () => void; onEditPermissions: () => void;
}) {
  return <div role="status" className="ct-card ct-card--static neighbor-missing">
    <div className="min-w-0 flex-1">
      <p className="ct-card__title">{missingConciergeLine(status)}</p>
      <p className="ct-card__meta">Set their permissions here and a concierge starts answering on this machine.</p>
    </div>
    <div className="neighbor-confirm__actions">
      <Button size="sm" variant="primary" onClick={onSetUpWithGeneral}>Set up with General</Button>
      <Button size="sm" onClick={onEditPermissions}>Edit permissions</Button>
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
    <p className="neighbor-panel__lead">{request.name} wants their agents to be able to message yours. Your General Agent will ask what they may do once you accept.</p>
    <div className="neighbor-confirm__actions">
      <Button size="sm" variant="primary" disabled={pending} onClick={onAccept} aria-label={`Accept ${request.name}`}>Accept</Button>
      <Button size="sm" disabled={pending} onClick={onDecline} aria-label={`Decline ${request.name}`}>Decline</Button>
    </div>
  </aside>;
}
