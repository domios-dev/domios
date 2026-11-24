import { daemonTransportForPeer } from "./daemonBootstrap";
import { TransportError } from "./transportClient";
import { listen } from "./bridge";
import { relayTarget } from "./webApi";
import type { AppearanceLook, AppearanceProposal, ContrastCheck, CurrentAppearance, ThemeSwitchReceipt } from "./appearance";

export const APPEARANCE_CHANGED = "codeterm:appearance-changed";
export const OPEN_APPEARANCE = "codeterm:open-appearance";
export interface AppearanceTarget { machine: string; peer?: string }
export interface OpenAppearance extends AppearanceTarget { proposal: string }
export interface AppearanceCommit {
  event: string;
  machine: string;
  proposal: string | null;
  undo: boolean;
  look: AppearanceLook;
}

export async function listenAppearanceCommit(handler: (commit: AppearanceCommit) => void, peer?: string) {
  if (peer && peer !== relayTarget()?.id) {
    return daemonTransportForPeer(peer).subscribe("/api/events/stream", (name, data) => {
      if (name === "appearance-committed") handler(data as AppearanceCommit);
    });
  }
  return listen<AppearanceCommit>("appearance-committed", event => handler(event.payload));
}

function path(target: AppearanceTarget, id?: string): string {
  const root = `/api/appearance/${encodeURIComponent(target.machine)}`;
  return id ? `${root}/${encodeURIComponent(id)}` : root;
}

export async function readAppearance(target: AppearanceTarget): Promise<CurrentAppearance> {
  return daemonTransportForPeer(target.peer).json("GET", path(target));
}

export async function readAppearanceProposal(target: AppearanceTarget, id: string): Promise<AppearanceProposal> {
  return daemonTransportForPeer(target.peer).json("GET", path(target, id));
}

export async function proposeAppearance(target: AppearanceTarget, look: AppearanceLook, expected: AppearanceLook): Promise<AppearanceProposal> {
  return daemonTransportForPeer(target.peer).json("POST", path(target), { look, expected, rationale: "Edited in Appearance Lab" });
}

export async function checkAppearanceContrast(target: AppearanceTarget, look: AppearanceLook, signal: AbortSignal): Promise<ContrastCheck[]> {
  return daemonTransportForPeer(target.peer).json("POST", `${path(target)}/validate`, look, { signal });
}

export async function previewAppearancePreset(target: AppearanceTarget, name: string, expected: AppearanceLook): Promise<AppearanceLook> {
  return daemonTransportForPeer(target.peer).json("POST", `${path(target)}/preview-preset`, { name, expected });
}

export async function switchAppearanceTheme(
  target: AppearanceTarget,
  selection: { kind: "settings"; name: string } | { kind: "preset"; name: string; expected: AppearanceLook },
): Promise<ThemeSwitchReceipt | null> {
  const receipt = await daemonTransportForPeer(target.peer).json<ThemeSwitchReceipt | null>("POST", `${path(target)}/switch-theme`, selection);
  if (receipt) publishAppearanceChange(target, receipt);
  return receipt;
}

export async function changeAppearance(target: AppearanceTarget, id: string, action: "apply" | "undo" | "use-theme-wallpaper"): Promise<AppearanceProposal> {
  const receipt = await daemonTransportForPeer(target.peer).json<AppearanceProposal>("POST", `${path(target, id)}/${action}`);
  publishAppearanceChange(target, receipt);
  return receipt;
}

function publishAppearanceChange(target: AppearanceTarget, receipt: AppearanceProposal) {
  window.dispatchEvent(new CustomEvent(APPEARANCE_CHANGED, { detail: { target, receipt } }));
}

export function appearanceError(error: unknown): string {
  if (!(error instanceof TransportError)) return error instanceof Error ? error.message : String(error);
  try {
    const data = JSON.parse(error.body);
    return typeof data?.message === "string" ? data.message : error.body;
  } catch {
    return error.body;
  }
}
