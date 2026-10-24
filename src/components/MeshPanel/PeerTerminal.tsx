import { PeerCapsuleSections } from "./PeerCapsuleSections";
import type { PeerInventory } from "./peerInventoryModel";
import { memo, type ReactNode } from "react";
import { MachineName } from "./MachineName";

type AccessLevel = "full" | "read_only" | "agent_isolated";

interface Props {
  inventory?: PeerInventory;
  name: string;
  ip: string;
  isThisMachine?: boolean;
  isPaired?: boolean;
  /** Vouched-by-a-paired-peer but not yet paired itself — must never render as "paired". */
  pending?: boolean;
  accessLevel?: AccessLevel;
  pairedDuration?: string;
  onClick?: () => void;
  offline?: boolean;
  disabled?: boolean;
  onGoOnline?: () => void;
  footer?: ReactNode;
  onRename?: (name: string) => Promise<boolean>;
  renameDisabledReason?: string;
}

function PeerTerminalComponent({
  inventory,
  name,
  ip,
  isThisMachine,
  isPaired,
  pending,
  accessLevel,
  pairedDuration,
  onClick,
  offline = false,
  disabled = false,
  onGoOnline,
  footer,
  onRename,
  renameDisabledReason,
}: Props) {
  const dim = offline ? "opacity-55 grayscale" : "";
  return (
    <div
      role="group"
      aria-label={name}
      onClick={disabled ? undefined : onClick}
      className={`relative bg-[var(--ct-bg)] rounded-md cursor-pointer transition-shadow ${
        isThisMachine
          ? offline
            ? "border border-[var(--ct-border)] shadow-none"
            : "border border-[color-mix(in_srgb,var(--ct-accent)_72%,var(--ct-border))] shadow-[var(--ct-shadow-capsule)]"
          : "border border-[var(--ct-border)] shadow-sm"
      }`}
      style={{ width: 360 }}
    >
      <div className={`flex items-center gap-2 px-3.5 py-2 rounded-t-md ${dim}`}>
        <span role="button" tabIndex={disabled ? -1 : 0} aria-disabled={disabled} aria-label={`Open ${name}`}
          onKeyDown={(event) => { if (event.code === "Enter" || event.code === "Space") { event.preventDefault(); if (!disabled) onClick?.(); } }}
          className="flex items-center gap-2 rounded focus-visible:outline-2 focus-visible:outline-[var(--ct-accent)]">
          <span className="w-3 h-3 shrink-0 rounded-full bg-[var(--ct-window-close)]" />
          <span className="w-3 h-3 shrink-0 rounded-full bg-[var(--ct-window-minimize)]" />
          <span className="w-3 h-3 shrink-0 rounded-full bg-[var(--ct-window-maximize)]" />
        </span>
        <span className="ml-2 min-w-0 flex-1 text-[14px] text-[var(--ct-fg)] font-medium truncate">
          <MachineName name={name} onRename={onRename} disabledReason={renameDisabledReason} />
        </span>
        <span className="ml-auto max-w-[45%] truncate text-[11px] text-[var(--ct-muted)] font-mono whitespace-nowrap">
          {ip}
          {isThisMachine ? (offline ? " · offline" : " · this") : ""}
        </span>
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
      <div
        className={`px-3.5 py-2 text-[12px] ${dim}`}
        style={{
          color: "var(--ct-muted-fg)",
        }}
      >
        {inventory && <PeerCapsuleSections inventory={inventory} />}
        {isPaired && accessLevel && (
          <div className="mt-2 text-[10px] text-[var(--ct-muted)]">
            paired · access: {accessLevel}
            {pairedDuration ? ` · ${pairedDuration}` : ""}
          </div>
        )}
        {!isPaired && pending && (
          <div className="mt-2 text-[10px] text-[var(--ct-muted)]">
            pending · auto-pairs when online
          </div>
        )}
      </div>
      {footer}
    </div>
  );
}

export const PeerTerminal = memo(PeerTerminalComponent);
