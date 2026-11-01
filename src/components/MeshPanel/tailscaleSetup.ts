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
  | { kind: "permission_needed" }
  | { kind: "signed_out" }
  | { kind: "connected"; tailnet: string | null };

export const TAILSCALE_DOWNLOAD_URL = "https://tailscale.com/download";

export interface TailscalePrompt { title: string; message: string }

/** What this machine needs from the user, in plain words; null when nothing does (working, checking or unknown). */
export function tailscalePrompt(state: TailscaleState): TailscalePrompt | null {
  switch (state.kind) {
    case "not_installed":
      return { title: "Connect this computer", message: "Domios links your computers through a free, encrypted private network called Tailscale. Nothing is exposed to your Wi-Fi or the internet." };
    case "not_running":
      return { title: "Private network is off", message: "Tailscale is installed but not running on this computer, so your other machines can't reach it." };
    case "permission_needed":
      return { title: "Permission needed", message: "Domios could not read the Tailscale connection because access was denied. Allow Domios to access Tailscale data in macOS Privacy & Security settings, then check again." };
    case "signed_out":
    case "not_connected":
      return { title: "Sign in to your private network", message: "Sign in to Tailscale with the same account you use on your other computers." };
    default:
      return null;
  }
}

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
    "Check the Tailscale state yourself first; if it is already installed, running and signed in to the right account, say so and stop.",
    "Otherwise explain in one or two plain sentences what Tailscale is and why Domios uses it, without networking jargon.",
    "If it is missing, offer to install it with the platform's package manager (winget on Windows, Homebrew on macOS, the official package or script on Linux) and ask before installing anything.",
    "Walk me through signing in, remind me to use the same Tailscale account on every machine, and finish by confirming that Domios Mesh sees the Tailscale connection.",
    "After setup, run `domios mesh refresh --json`. Read back its transport and web_ui fields, then `domios mesh status --json`, and verify the Mesh card reloads the connected state and offers Open, Copy and QR for a verified endpoint. Do not claim completion from Tailscale output alone; report any remaining card or endpoint failure.",
    "Reply in my language.",
  ].join(" ");
}

export function startTailscaleSetup(machine: string | null, api: DaemonApiConfig): Promise<AiHelperSpawned> {
  return spawnAiHelper({ title: "Tailscale", description: "Set up Tailscale for Mesh", task: withAiHelperContext(tailscaleSetupRequest(machine)) }, api);
}
