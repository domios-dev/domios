import type { Service } from "./types";

type AccessLevel = "full" | "read_only" | "agent_isolated";

interface Props {
  name: string;
  ip: string;
  isThisMachine?: boolean;
  isPaired?: boolean;
  accessLevel?: AccessLevel;
  pairedDuration?: string;
  services: Service[];
  onClick?: () => void;
  /** Dim the card + show a "Go online" sticker. Only meaningful on the
   * self card when mesh broadcasting is disabled. */
  offline?: boolean;
  onGoOnline?: () => void;
}

const HEALTH_DOT_COLOR: Record<string, string> = {
  healthy: "var(--ct-green)",
  degraded: "var(--ct-yellow)",
  down: "var(--ct-red)",
  unknown: "var(--ct-muted)",
};

const MAX_SERVICES_ON_CARD = 5;

export function PeerTerminal({
  name,
  ip,
  isThisMachine,
  isPaired,
  accessLevel,
  pairedDuration,
  services,
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
            ? "border-2 border-[var(--ct-border)] shadow-none opacity-55 grayscale"
            : "border-2 border-[var(--ct-accent)] shadow-[0_8px_24px_rgba(203,166,247,0.2)]"
          : "border border-[var(--ct-border)] shadow-md"
      }`}
      style={{ width: 360 }}
    >
      <div className="flex items-center gap-2 px-3.5 py-2 bg-[var(--ct-titlebar-bg)] border-b border-[var(--ct-border)] rounded-t-md">
        <span className="w-3 h-3 rounded-full bg-[#f38ba8]" />
        <span className="w-3 h-3 rounded-full bg-[#f9e2af]" />
        <span className="w-3 h-3 rounded-full bg-[#a6e3a1]" />
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
        <div className="text-[var(--ct-muted)] mb-1">
          $ codeterm services list
        </div>
        <div className="flex gap-1 flex-wrap">
          {services.slice(0, MAX_SERVICES_ON_CARD).map((s) => (
            <span
              key={s.id}
              className="bg-[var(--ct-titlebar-bg)] px-2 py-0.5 rounded text-[11px] inline-flex items-center gap-1.5"
              style={{ color: HEALTH_DOT_COLOR[s.health.status] ?? HEALTH_DOT_COLOR.unknown }}
            >
              <span
                className="w-[7px] h-[7px] rounded-full"
                style={{ background: HEALTH_DOT_COLOR[s.health.status] ?? HEALTH_DOT_COLOR.unknown }}
              />
              {s.name}
              <span className="opacity-50">·{s.kind}</span>
            </span>
          ))}
          {services.length > MAX_SERVICES_ON_CARD && (
            <span className="text-[var(--ct-muted)] text-[11px] italic">
              +{services.length - MAX_SERVICES_ON_CARD} more…
            </span>
          )}
          {services.length === 0 && (
            <span className="text-[var(--ct-muted)] italic text-[11px]">
              no services
            </span>
          )}
        </div>
        {isPaired && accessLevel && (
          <div className="mt-2 text-[10px] text-[var(--ct-muted)]">
            paired · access: {accessLevel}
            {pairedDuration ? ` · ${pairedDuration}` : ""}
          </div>
        )}
      </div>
    </div>
  );
}
