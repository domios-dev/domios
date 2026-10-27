import { useId, useState, type ReactNode } from "react";
import { Lock, MessageSquare, MessagesSquare, SlidersHorizontal, SquareArrowOutUpRight, UserX, X } from "lucide-react";
import { AccountAvatar } from "../Titlebar/AccountAvatar";
import { accountInitials } from "../Settings/accountModel";
import { Button } from "../shared/Button";
import type { Neighbor, NeighborRequest } from "./neighborModel";
import { NeighborChannelView, type NeighborChannelState } from "./NeighborChannel";
import { NeighborPermissionsEditor } from "./NeighborPermissionsEditor";
import { missingConciergeLine, type ConciergeStatus } from "./useConciergeStatus";
import type { NeighborPermission } from "./neighborRuntimeClient";
import { STATUS_LABEL, type NeighborNodeStatus } from "./neighborhoodModel";

type Section = "messages" | "configure" | "revoke";

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

function ActionRow({ icon, label, hint, expanded, disabled, danger, onClick }: {
  icon: ReactNode; label: string; hint?: string; expanded?: boolean; disabled?: boolean; danger?: boolean; onClick: () => void;
}) {
  const hintId = useId();
  return <button type="button" className="neighbor-action ct-liquid-interactive" data-danger={danger || undefined} aria-expanded={expanded} disabled={disabled}
    aria-label={label} aria-describedby={hint ? hintId : undefined} onClick={onClick}>
    <span className="neighbor-action__icon" aria-hidden="true">{icon}</span>
    <span className="min-w-0 flex-1">
      <span className="neighbor-action__label">{label}</span>
      {hint && <span id={hintId} className="neighbor-action__hint">{hint}</span>}
    </span>
  </button>;
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

export function NeighborPanel({ neighbor, name, locked, pending, channel, gateway, initialSection = null, concierge = null, onOpenGateway, onTalkAbout, onInterview, onSavePermission, onSaveDailyLimit, onLimit, onRemove, onPlans, onClose }: {
  neighbor: Neighbor; name: string; locked: boolean; pending: boolean; initialSection?: "configure" | null;
  concierge?: ConciergeStatus | null;
  channel: NeighborChannelState | null;
  gateway: { title: string } | null;
  onOpenGateway: () => void; onTalkAbout: () => void; onInterview: () => void;
  onSavePermission: (permission: NeighborPermission) => Promise<void>;
  onSaveDailyLimit: (dailyLimit: number | null) => Promise<void>;
  onLimit: (limit: number) => Promise<boolean>; onRemove: () => Promise<boolean>; onPlans: () => void; onClose: () => void;
}) {
  const [open, setOpen] = useState<Section | null>(initialSection);
  const toggle = (section: Section) => setOpen(current => current === section ? null : section);
  const permission = channel?.permission;
  return <aside className="neighbor-panel" aria-label={name}>
    <PanelHead name={name} photoURL={neighbor.photoURL} status={locked ? "paused" : "connected"}
      meta={`since ${new Date(neighbor.since * 1000).toLocaleDateString()}`} onClose={onClose} />
    {concierge?.concierge_missing && <MissingConciergeNotice status={concierge} onSetUpWithGeneral={onInterview} onEditPermissions={() => setOpen("configure")} />}
    {neighbor.receiving === false && <p role="status" className="neighbor-note">Not receiving right now</p>}
    {locked && <div role="status" className="ct-card ct-card--static neighbor-perk">
      <div className="min-w-0 flex-1">
        <p className="ct-card__title">Messages are paused by your plan</p>
        <p className="ct-card__meta">You can keep and manage this connection.</p>
      </div>
      <Button size="sm" variant="accent" onClick={onPlans}>View plans</Button>
    </div>}
    <nav aria-label={`Actions for ${name}`} className="neighbor-actions">
      <ActionRow icon={<MessageSquare size={15} />} label="Messages" hint="Send and check encrypted messages" expanded={open === "messages"} onClick={() => toggle("messages")} />
      {open === "messages" && <div className="neighbor-panel__section">{channel ? <NeighborChannelView channel={channel} friendship={neighbor.friendship_id} locked={locked} /> : null}</div>}
      <ActionRow icon={<SquareArrowOutUpRight size={15} />} label="Open their tab" disabled={!gateway}
        hint={gateway ? `Their messages arrive in ${gateway.title}` : "No tab receives their messages yet"} onClick={onOpenGateway} />
      <ActionRow icon={<MessagesSquare size={15} />} label={`Talk about ${name}`} hint="Opens your General Agent with them in mind" onClick={onTalkAbout} />
      <ActionRow icon={<SlidersHorizontal size={15} />} label="Configure" hint={permission ? (permission.allowed ? "Some requests allowed" : "Nothing allowed yet") : "What they may ask for"}
        expanded={open === "configure"} onClick={() => toggle("configure")} />
      {open === "configure" && <div className="neighbor-panel__section">
        <NeighborPermissionsEditor neighbor={neighbor} name={name} permission={permission} pending={pending} onSave={onSavePermission} onSaveDailyLimit={onSaveDailyLimit} onLimit={onLimit} onEditWithGeneral={onInterview} />
      </div>}
      <ActionRow icon={<UserX size={15} />} label="Revoke" hint="End their access to your agents" danger expanded={open === "revoke"} onClick={() => toggle("revoke")} />
      {open === "revoke" && <div role="group" aria-label="Confirm removal" className="neighbor-panel__section neighbor-confirm">
        <p>Revoke {name}? Their agents lose access to yours, and you would need a new request to reconnect.</p>
        <div className="neighbor-confirm__actions">
          <Button size="sm" variant="danger" disabled={pending} onClick={() => { void onRemove().then(removed => { if (removed) onClose(); }); }}>Confirm revoke</Button>
          <Button size="sm" disabled={pending} onClick={() => setOpen(null)}>Keep Neighbor</Button>
        </div>
      </div>}
    </nav>
    <NeighborPrivacyNote className="neighbor-panel__privacy" />
  </aside>;
}
