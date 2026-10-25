import { ShieldCheck, ShieldAlert } from "lucide-react";
import { UI_BUTTON, UI_TEXT } from "../shared/design";
import { BusySpinner } from "../shared/BusyState";
import { tailscalePrompt, type TailscaleState } from "./tailscaleSetup";
import type { NetworkIdentity } from "./networkTransport";

export interface CanvasTailscale {
  state: TailscaleState;
  identity?: NetworkIdentity | null;
  launching: boolean;
  /** Opens the General agent with the setup request; `machine` names an unreachable machine, null means this one. */
  onSetup: (machine: string | null) => void;
  onInstall: () => void;
}

const MANUAL_LINK = "text-[12px] text-[var(--ct-muted-fg)] underline underline-offset-2 decoration-[var(--ct-border)] hover:text-[var(--ct-fg)]";

function Actions({ tailscale, machine, ariaLabel }: { tailscale: CanvasTailscale; machine: string | null; ariaLabel?: string }) {
  return (
    <div className="flex flex-wrap items-center gap-3 pt-0.5">
      <button type="button" onClick={() => tailscale.onSetup(machine)} disabled={tailscale.launching} aria-busy={tailscale.launching}
        aria-label={ariaLabel} className={`${UI_BUTTON.primary} ${UI_BUTTON.compact}`}>
        {tailscale.launching ? <><BusySpinner size={12} /> Opening…</> : "Set up with General agent"}
      </button>
      <button type="button" onClick={tailscale.onInstall} className={MANUAL_LINK}>Set up manually</button>
    </div>
  );
}

/** The private network under this machine's card: a quiet mark when it works, one friendly prompt when it needs the user. */
export function TailscaleFooter({ tailscale }: { tailscale: CanvasTailscale }) {
  const { state } = tailscale;
  if (state.kind === "connected") {
    return (
      <div role="status" className="flex items-center gap-1.5 border-t border-[var(--ct-border)] px-3.5 py-2 text-[12px] text-[var(--ct-muted-fg)]">
        <ShieldCheck size={13} className="shrink-0 text-[var(--ct-green)]" aria-hidden="true" />
        Private network on
      </div>
    );
  }
  const prompt = tailscalePrompt(state);
  if (!prompt) return null;
  return (
    <section aria-label={prompt.title} className="flex flex-col gap-1.5 border-t border-[var(--ct-border)] px-3.5 py-3" onClick={(event) => event.stopPropagation()}>
      <div className="flex items-center gap-2 min-w-0">
        <ShieldAlert size={14} className="shrink-0 text-[var(--ct-accent)]" aria-hidden="true" />
        <span className="text-[13px] font-medium text-[var(--ct-fg)]">{prompt.title}</span>
      </div>
      <p className="m-0 text-[12px] leading-relaxed text-[var(--ct-muted-fg)]">{prompt.message}</p>
      <Actions tailscale={tailscale} machine={null} />
    </section>
  );
}

/** On a machine this one cannot reach: the likely cause, said once, with the same two ways forward. */
export function TailscaleMachineHint({ name, tailscale, detail }: { name: string; tailscale: CanvasTailscale; detail?: string }) {
  return (
    <div role="note" className="flex flex-col gap-1.5 rounded-md border border-[var(--ct-border)] bg-[color-mix(in_srgb,var(--ct-accent)_4%,var(--ct-bg))] px-3 py-2.5" onClick={(event) => event.stopPropagation()}>
      <span className="text-[12px] font-medium text-[var(--ct-fg)]">{detail ? "On another private network" : `Can't reach ${name}`}</span>
      <span className={UI_TEXT.metaMuted}>{detail ?? "Make sure Tailscale is running there and signed in with the same account as this computer."}</span>
      <Actions tailscale={tailscale} machine={name} ariaLabel={`Set up the private network on ${name} with General agent`} />
    </div>
  );
}
