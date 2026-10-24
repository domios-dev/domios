import { spawnAiHelper, withAiHelperContext, type AiHelperSpawned } from "../../lib/aiHelperSpawn";
import type { DaemonApiConfig } from "../../lib/webApi";
import type { ServeStatus } from "../../lib/webAccess";
import { transportState } from "./networkTransport";
import type { NetworkInfo } from "./webUiTarget";

export type TailscaleState =
  | { kind: "checking" }
  | { kind: "unknown" }
  | { kind: "not_installed" }
  | { kind: "not_connected" }
  | { kind: "not_running" }
  | { kind: "signed_out" }
  | { kind: "connected"; tailnet: string | null };

export const TAILSCALE_COPY = "Domios connects your machines through Tailscale, a free private network. Install it on each machine and sign in with the same Tailscale account.";
export const TAILSCALE_WHY = "It is private and encrypted, works across networks, and exposes nothing to your Wi-Fi or the internet.";
export const TAILSCALE_DOWNLOAD_URL = "https://tailscale.com/download";

/** The tailnet is the MagicDNS suffix after this machine's own label: "box.tail1234.ts.net." -> "tail1234.ts.net". */
export function tailnetName(selfDns: string | null | undefined): string | null {
  const tailnet = selfDns?.replace(/\.$/, "").split(".").slice(1).join(".") ?? "";
  return tailnet.includes(".") && tailnet !== "ts.net" ? tailnet : null;
}

/** A Tailscale address means this machine is up and signed in; the CLI probe tells a missing install from a stopped one. */
export function tailscaleState({ serve, network, loading }: { serve: ServeStatus | null; network: NetworkInfo | null; loading: boolean }): TailscaleState {
  if (network?.transport) return transportState(network.transport);
  if (network?.tailscale_ip) return { kind: "connected", tailnet: tailnetName(serve?.self_dns) };
  if (loading) return { kind: "checking" };
  if (!serve) return { kind: "unknown" };
  return serve.installed ? { kind: "not_connected" } : { kind: "not_installed" };
}

export function tailscaleReady(state: TailscaleState): boolean {
  return state.kind === "connected" || state.kind === "checking" || state.kind === "unknown";
}

/** An unreachable account machine points at its own Tailscale only when this machine's side is known to be fine. */
export function suggestsRemoteTailscale(online: boolean, state: TailscaleState): boolean {
  return !online && state.kind === "connected";
}

/** The one request the General agent receives; it explains, checks, installs with consent and verifies on its own. */
export function tailscaleSetupRequest(machine: string | null): string {
  const subject = machine === null
    ? "Help me set up Tailscale on this machine so Domios Mesh can connect my machines."
    : `Domios Mesh cannot reach my machine "${machine}". Help me check Tailscale on it.`;
  return [
    subject,
    "Briefly explain what Tailscale is and why Domios uses it, then check the Tailscale state yourself.",
    "If it is missing, offer to install it with the platform's package manager (winget on Windows, Homebrew on macOS, the official package or script on Linux) and ask before installing anything.",
    "Walk me through signing in, remind me to use the same Tailscale account on every machine, and finish by confirming that Domios Mesh sees the Tailscale connection.",
    "Reply in my language.",
  ].join(" ");
}

export function startTailscaleSetup(machine: string | null, api: DaemonApiConfig): Promise<AiHelperSpawned> {
  return spawnAiHelper({ title: "Tailscale", description: "Set up Tailscale for Mesh", task: withAiHelperContext(tailscaleSetupRequest(machine)) }, api);
}
