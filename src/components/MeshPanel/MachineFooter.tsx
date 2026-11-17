import type { ReactNode } from "react";
import { ShieldCheck } from "lucide-react";
import { appVersion, buildHash } from "../../lib/buildStamp";
import { BusyState } from "../shared/BusyState";
import type { MachineTone } from "./accountCanvas";
import { privateNetworkLabel, type TailscaleState } from "./tailscaleSetup";

export const ACCESS_LABELS: Record<string, string> = {
  full: "Full access",
  read_only: "Read only",
  agent_isolated: "Its own agents only",
};

const TONE_COLOR: Record<MachineTone, string> = {
  ok: "var(--ct-green)", progress: "var(--ct-accent)", muted: "var(--ct-muted-fg)",
  warning: "var(--ct-yellow)", danger: "var(--ct-red)",
};

export function MachineVersion({ version, build, localVersion = appVersion(), showBuild }: { version?: string | null; build?: string | null; localVersion?: string; showBuild?: boolean }) {
  if (!version) return null;
  const mismatch = version !== localVersion;
  return <span className="ml-auto flex shrink-0 items-center gap-1.5 whitespace-nowrap text-[11px] text-[var(--ct-muted-fg)]" title={build ? `Build ${build}` : undefined}>
    {mismatch && <span role="img" aria-label={`This machine runs ${localVersion}`} title={`This machine runs ${localVersion}`} className="h-1.5 w-1.5 shrink-0 rounded-full bg-[var(--ct-yellow)]" />}
    Domios {version}{showBuild && build ? ` · build ${build}` : ""}
  </span>;
}

export function MachineFooter({ label, secondary, tone = "muted", connecting, version, build, children, menu, statusIcon, showBuild, inset = true }: {
  label: string | null; secondary?: string | null; tone?: MachineTone; connecting?: boolean;
  version?: string | null; build?: string | null; children?: ReactNode; menu?: ReactNode;
  statusIcon?: ReactNode; showBuild?: boolean; inset?: boolean;
}) {
  return <div className={`flex min-h-10 min-w-0 items-center gap-2 border-t text-[11px] text-[var(--ct-muted-fg)] ${inset ? "px-3.5" : ""}`} style={{ borderColor: "color-mix(in srgb, var(--ct-border) 55%, transparent)" }}>
    <div role="status" aria-busy={connecting || undefined} className="flex min-w-0 flex-1 items-center gap-1.5">
      {label !== null && <>
        {connecting ? <BusyState inline hideLabel label={label} phase="work" dotSize="micro" /> : statusIcon ?? <span aria-hidden="true" className="h-1.5 w-1.5 shrink-0 rounded-full" style={{ background: TONE_COLOR[tone] }} />}
        <span className="truncate whitespace-nowrap text-[var(--ct-fg)]">{label}{secondary && <span className="text-[var(--ct-muted-fg)]"> · {secondary}</span>}</span>
      </>}
    </div>
    {children}
    <MachineVersion version={version} build={build} showBuild={showBuild} />
    {inset && <div className="flex w-[26px] shrink-0 items-center justify-end">{menu}</div>}
  </div>;
}

export function LocalMachineFooter({ state }: { state?: TailscaleState }) {
  const networkOn = state?.kind === "connected";
  return <MachineFooter label={privateNetworkLabel(state)} tone={networkOn ? "ok" : "muted"} version={appVersion()} build={buildHash()}
    statusIcon={<ShieldCheck size={13} className={`shrink-0 ${networkOn ? "text-[var(--ct-green)]" : "text-[var(--ct-muted-fg)]"}`} aria-hidden="true" />} />;
}
