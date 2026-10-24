import { Network } from "lucide-react";
import { UI_BUTTON, UI_TEXT } from "../shared/design";
import { BusySpinner } from "../shared/BusyState";
import { SettingsPill } from "../Settings/shared";
import { TAILSCALE_COPY, TAILSCALE_WHY, tailscaleReady, type TailscaleState } from "./tailscaleSetup";

export interface CanvasTailscale {
  state: TailscaleState;
  identity?: import("./networkTransport").NetworkIdentity | null;
  launching: boolean;
  /** Opens the General agent with the setup request; `machine` names an unreachable machine, null means this one. */
  onSetup: (machine: string | null) => void;
  onInstall: () => void;
}

const STATUS: Record<TailscaleState["kind"], { label: string; tone: "green" | "amber" | "muted" }> = {
  connected: { label: "Private network on", tone: "green" },
  not_running: { label: "Not running", tone: "amber" },
  signed_out: { label: "Signed out", tone: "amber" },
  not_connected: { label: "Not signed in", tone: "amber" },
  not_installed: { label: "Not installed", tone: "amber" },
  checking: { label: "Checking", tone: "muted" },
  unknown: { label: "Unknown", tone: "muted" },
};

function SetupButton({ label, launching, onClick }: { label: string; launching: boolean; onClick: () => void }) {
  return (
    <button type="button" onClick={onClick} disabled={launching} aria-busy={launching}
      className={`${UI_BUTTON.primary} ${UI_BUTTON.compact}`}>
      {launching ? <><BusySpinner size={12} /> Opening…</> : label}
    </button>
  );
}

/** Tailscale on this machine, under its card: a quiet status when connected, the explanation and one way forward otherwise. */
export function TailscaleFooter({ tailscale }: { tailscale: CanvasTailscale }) {
  const { state } = tailscale;
  const status = STATUS[state.kind];
  if (state.kind === "connected") {
    return <section aria-label="Private network on" className="px-3.5 py-2 text-[12px] text-[var(--ct-muted-fg)]">Private network on</section>;
  }
  return (
    <section aria-label="Tailscale on this machine" className="flex flex-col gap-2 border-t border-[var(--ct-border)] px-3.5 py-2.5" onClick={(event) => event.stopPropagation()}>
      <div className="flex items-center gap-2 min-w-0">
        <Network size={14} className="shrink-0 text-[var(--ct-muted-fg)]" aria-hidden="true" />
        <span className="text-[13px] font-medium text-[var(--ct-fg)]">Tailscale</span>
        <SettingsPill tone={status.tone}>{status.label}</SettingsPill>
      </div>
      {!tailscaleReady(state) && (
        <>
          <p className="m-0 text-[12px] text-[var(--ct-muted-fg)]">{TAILSCALE_COPY}</p>
          <p className="m-0 text-[12px] text-[var(--ct-muted-fg)]"><span className="text-[var(--ct-fg)]">Why Tailscale?</span> {TAILSCALE_WHY}</p>
          <div className="flex flex-wrap items-center gap-3 pt-0.5">
            <SetupButton label="Set up with General agent" launching={tailscale.launching} onClick={() => tailscale.onSetup(null)} />
            <button type="button" onClick={tailscale.onInstall} className="text-[12px] text-[var(--ct-accent)] underline-offset-2 hover:underline">Install Tailscale</button>
          </div>
        </>
      )}
    </section>
  );
}

/** On an unreachable machine while this one is on Tailscale: the likely cause is that machine's Tailscale. */
export function TailscaleMachineHint({ name, tailscale, detail }: { name: string; tailscale: CanvasTailscale; detail?: string }) {
  return (
    <div role="note" className="flex flex-col gap-1.5 rounded border border-[var(--ct-border)] px-2.5 py-2" onClick={(event) => event.stopPropagation()}>
      <span className="text-[12px] text-[var(--ct-fg)]">{detail ?? `Check the private network on ${name}`}</span>
      <span className={UI_TEXT.metaMuted}>It must be running and signed in to the same Tailscale account as this machine.</span>
      <div>
        <button type="button" onClick={() => tailscale.onSetup(name)} disabled={tailscale.launching} aria-busy={tailscale.launching}
          aria-label={`Ask General agent about Tailscale on ${name}`} className={`${UI_BUTTON.ghost} ${UI_BUTTON.compact} -ml-2`}>
          Set up with General agent
        </button>
      </div>
    </div>
  );
}
