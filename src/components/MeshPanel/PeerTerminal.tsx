import { memo } from "react";
import { ArrowUpRight, Globe } from "lucide-react";
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
  offline?: boolean;
  onGoOnline?: () => void;
  servicesStale?: boolean;
  servicesError?: string | null;
  servicesLoaded?: boolean;
  /** Verified peer web UI URL — the Open action shows only when this is set. */
  webUrl?: string | null;
  onOpenWebUi?: (url: string) => void;
}

const HEALTH_DOT_COLOR: Record<string, string> = {
  healthy: "var(--ct-green)",
  degraded: "var(--ct-yellow)",
  down: "var(--ct-red)",
  unknown: "var(--ct-muted)",
};

const MAX_SERVICES_ON_CARD = 5;

function PeerTerminalComponent({
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
  webUrl = null,
  onOpenWebUi,
}: Props) {
  const visibleServices = services.filter((service) => service.kind !== "cron");
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
          {visibleServices.slice(0, MAX_SERVICES_ON_CARD).map((s) => (
            <span
              key={`${s.source_peer_id ?? s.source_peer ?? "local"}:${s.id}`}
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
          {visibleServices.length > MAX_SERVICES_ON_CARD && (
            <span className="text-[var(--ct-muted)] text-[11px] italic">
              +{visibleServices.length - MAX_SERVICES_ON_CARD} more…
            </span>
          )}
          {visibleServices.length === 0 && (
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
      <div className="flex items-center gap-2 border-t border-[var(--ct-border)] px-3.5 py-2">
        <span className="flex h-5 w-5 items-center justify-center rounded bg-[color-mix(in_srgb,var(--ct-accent)_10%,transparent)] text-[var(--ct-accent)]">
          <Globe size={11} />
        </span>
        <span className="text-[11px] font-medium text-[var(--ct-fg)]">Web UI</span>
        <button
          type="button"
          data-testid="peer-web-ui-open"
          data-peer-web-url={webUrl ?? undefined}
          disabled={!webUrl}
          onClick={(e) => {
            e.stopPropagation();
            if (webUrl) onOpenWebUi?.(webUrl);
          }}
          className="ml-auto inline-flex h-6 items-center gap-1 rounded border border-[color-mix(in_srgb,var(--ct-accent)_45%,var(--ct-border))] bg-[color-mix(in_srgb,var(--ct-accent)_10%,transparent)] px-2.5 text-[10px] font-semibold text-[var(--ct-accent)] transition-colors enabled:cursor-pointer enabled:hover:border-[var(--ct-accent)] enabled:hover:bg-[color-mix(in_srgb,var(--ct-accent)_18%,transparent)] disabled:cursor-default disabled:border-[var(--ct-border)] disabled:bg-[var(--ct-titlebar-bg)] disabled:text-[var(--ct-muted)]"
          title={webUrl ? `Open ${name}'s Web UI — ${webUrl}` : `${name} has not shared a Web UI`}
        >
          Open
          <ArrowUpRight size={11} />
        </button>
      </div>
    </div>
  );
}

export const PeerTerminal = memo(PeerTerminalComponent);
