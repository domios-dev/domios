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
}

const HEALTH_DOT_COLOR: Record<string, string> = {
  healthy: "var(--ct-green)",
  degraded: "var(--ct-yellow)",
  down: "var(--ct-red)",
  unknown: "var(--ct-muted)",
};

export function PeerTerminal({
  name,
  ip,
  isThisMachine,
  isPaired,
  accessLevel,
  pairedDuration,
  services,
  onClick,
}: Props) {
  return (
    <div
      onClick={onClick}
      className={`bg-[var(--ct-bg)] rounded-md cursor-pointer transition-shadow ${
        isThisMachine
          ? "border-2 border-[var(--ct-accent)] shadow-[0_8px_24px_rgba(203,166,247,0.2)]"
          : "border border-[var(--ct-border)] shadow-md"
      }`}
      style={{ width: isThisMachine ? 280 : 230 }}
    >
      <div className="flex items-center gap-1.5 px-2.5 py-1.5 bg-[var(--ct-titlebar-bg)] border-b border-[var(--ct-border)] rounded-t-md">
        <span className="w-2.5 h-2.5 rounded-full bg-[#f38ba8]" />
        <span className="w-2.5 h-2.5 rounded-full bg-[#f9e2af]" />
        <span className="w-2.5 h-2.5 rounded-full bg-[#a6e3a1]" />
        <span className="ml-1.5 text-[11px] text-[var(--ct-fg)] font-medium truncate">
          {name}
        </span>
        <span className="ml-auto text-[9px] text-[var(--ct-muted)] font-mono whitespace-nowrap">
          {ip}
          {isThisMachine ? " · this" : ""}
        </span>
      </div>
      <div
        className="px-2.5 py-1.5 text-[10px]"
        style={{
          color: "var(--ct-muted-fg)",
          fontFamily: "ui-monospace, monospace",
        }}
      >
        <div className="text-[var(--ct-muted)] mb-1">
          $ codeterm services list
        </div>
        <div className="flex gap-1 flex-wrap">
          {services.map((s) => (
            <span
              key={s.id}
              className="bg-[var(--ct-titlebar-bg)] px-1.5 py-0.5 rounded text-[9px] inline-flex items-center gap-1"
              style={{ color: HEALTH_DOT_COLOR[s.health.status] ?? HEALTH_DOT_COLOR.unknown }}
            >
              <span
                className="w-[5px] h-[5px] rounded-full"
                style={{ background: HEALTH_DOT_COLOR[s.health.status] ?? HEALTH_DOT_COLOR.unknown }}
              />
              {s.name}
              <span className="opacity-50">·{s.kind}</span>
            </span>
          ))}
          {services.length === 0 && (
            <span className="text-[var(--ct-muted)] italic text-[9px]">
              no services
            </span>
          )}
        </div>
        {isPaired && accessLevel && (
          <div className="mt-1.5 text-[8px] text-[var(--ct-muted)]">
            paired · access: {accessLevel}
            {pairedDuration ? ` · ${pairedDuration}` : ""}
          </div>
        )}
      </div>
    </div>
  );
}
