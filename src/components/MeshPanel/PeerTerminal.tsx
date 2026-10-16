import { memo } from "react";

type AccessLevel = "full" | "read_only" | "agent_isolated";

interface Props {
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
  onGoOnline?: () => void;
}

function PeerTerminalComponent({
  name,
  ip,
  isThisMachine,
  isPaired,
  pending,
  accessLevel,
  pairedDuration,
  onClick,
  offline = false,
  onGoOnline,
}: Props) {
  return (
    <div
      onClick={onClick}
      className={`relative bg-[var(--ct-bg)] rounded-md cursor-pointer transition-shadow ${
        isThisMachine
          ? offline
            ? "border border-[var(--ct-border)] shadow-none opacity-55 grayscale"
            : "border border-[color-mix(in_srgb,var(--ct-accent)_72%,var(--ct-border))] shadow-[0_6px_18px_rgba(0,0,0,0.14)]"
          : "border border-[var(--ct-border)] shadow-sm"
      }`}
      style={{ width: 360 }}
    >
      <div className="flex items-center gap-2 px-3.5 py-2 rounded-t-md">
        <span className="w-3 h-3 rounded-full bg-[var(--ct-window-close)]" />
        <span className="w-3 h-3 rounded-full bg-[var(--ct-window-minimize)]" />
        <span className="w-3 h-3 rounded-full bg-[var(--ct-window-maximize)]" />
        <span className="ml-2 text-[14px] text-[var(--ct-fg)] font-medium truncate">
          {name}
        </span>
        <span className="ml-auto text-[11px] text-[var(--ct-muted)] font-mono whitespace-nowrap">
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
          className="absolute top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 z-10 px-4 py-2 rounded-full bg-[var(--ct-green,#22c55e)] text-white text-[12px] font-semibold uppercase tracking-wider shadow-[0_6px_18px_rgba(34,197,94,0.5)] hover:brightness-110 active:scale-95 transition-transform"
          style={{
            transform: "translate(-50%, -50%) rotate(-4deg)",
            filter: "drop-shadow(0 0 0.5px rgba(0,0,0,0.4))",
          }}
          title="Turn on mesh broadcasting"
        >
          Go online
        </button>
      )}
      <div
        className="px-3.5 py-2 text-[12px]"
        style={{
          color: "var(--ct-muted-fg)",
          fontFamily: "ui-monospace, monospace",
        }}
      >
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
    </div>
  );
}

export const PeerTerminal = memo(PeerTerminalComponent);
