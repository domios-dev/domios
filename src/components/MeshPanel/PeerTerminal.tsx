import { PeerCapsuleSections } from "./PeerCapsuleSections";
import type { PeerInventory } from "./peerInventoryModel";
import { memo, type ReactNode } from "react";
import { MachineName } from "./MachineName";
import { MachineGlyph } from "../shared/MachineGlyph";
import { machineLabel } from "../../lib/machineIdentity";
import { useMeshMachineIdentity } from "../../lib/useMeshMachineIdentities";
import { MachineSessionControls } from "./MachineSessionControls";

interface Props {
  inventory?: PeerInventory;
  name: string;
  ip: string;
  isThisMachine?: boolean;
  onClick?: () => void;
  offline?: boolean;
  disabled?: boolean;
  onGoOnline?: () => void;
  footer?: ReactNode;
  displayName?: string | null;
  sameNameOthers?: number;
  onRename?: (name: string) => Promise<boolean>;
  renameDisabledReason?: string;
  peerId?: string;
}

function PeerTerminalComponent({
  inventory,
  name,
  ip,
  isThisMachine,
  onClick,
  offline = false,
  disabled = false,
  onGoOnline,
  footer,
  onRename,
  displayName,
  sameNameOthers,
  renameDisabledReason,
  peerId,
}: Props) {
  const identity = useMeshMachineIdentity(peerId, true, isThisMachine ? undefined : name);
  const hasResources = inventory && (inventory.services.length > 0 || inventory.crons.length > 0);
  const dim = offline ? "opacity-55 grayscale" : "";
  return (
    <div
      role="group"
      aria-label={machineLabel(name, identity, offline ? "offline" : "online")}
      onClick={disabled ? undefined : onClick}
      className="ct-card relative cursor-pointer"
      style={{ width: 360 }}
    >
      <div className="ct-card__head">
        <span role="button" tabIndex={disabled ? -1 : 0} aria-disabled={disabled} aria-label={`Open ${machineLabel(name, identity, offline ? "offline" : "online")}`}
          onKeyDown={(event) => { if (event.code === "Enter" || event.code === "Space") { event.preventDefault(); if (!disabled) onClick?.(); } }}
          className="flex items-center gap-2 rounded focus-visible:outline-2 focus-visible:outline-[var(--ct-accent)]">
          <MachineGlyph {...identity} size="tile" />
        </span>
        <div className={`flex min-w-0 flex-1 ${isThisMachine ? "flex-col" : "items-center gap-2"}`}>
          <span className="min-w-0 flex-1 text-[14px] font-medium text-[var(--ct-fg)]">
            <MachineName name={name} displayName={displayName} onRename={onRename} disabledReason={renameDisabledReason} />
          </span>
          <span className={`truncate whitespace-nowrap font-mono text-[11px] text-[var(--ct-muted-fg)] ${isThisMachine ? "" : "max-w-[45%]"}`}>
            {ip}
            {isThisMachine ? (offline ? " · offline" : " · this") : ""}
          </span>
        </div>
        {isThisMachine && <span onClick={(event) => event.stopPropagation()}><MachineSessionControls name={name} busy={false} identityPeer={null} sameNameOthers={sameNameOthers} rename={onRename ? { displayName, save: onRename } : undefined} /></span>}
      </div>
      {offline && onGoOnline && (
        <button
          onClick={(e) => {
            e.stopPropagation();
            onGoOnline();
          }}
          className="absolute top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 z-10 px-4 py-2 rounded-full bg-[var(--ct-green)] text-[var(--ct-bg)] text-[12px] font-semibold uppercase tracking-wider shadow-[var(--ct-shadow-capsule)] hover:brightness-110 active:scale-95 transition-transform"
          style={{
            transform: "translate(-50%, -50%) rotate(-4deg)",
          }}
          title="Turn on mesh broadcasting"
        >
          Go online
        </button>
      )}
      {hasResources && <div
        className={`px-3.5 py-2 text-[12px] ${dim}`}
        style={{
          color: "var(--ct-muted-fg)",
        }}
      >
        {inventory && <PeerCapsuleSections inventory={inventory} />}
      </div>}
      {footer}
    </div>
  );
}

export const PeerTerminal = memo(PeerTerminalComponent);
