import { daemonTransportForPeer } from "./daemonBootstrap";
import { TransportError } from "./transportClient";
import { listen } from "./bridge";
import { relayTarget } from "./webApi";
import type { AppearanceLook, AppearanceProposal, ContrastCheck, CurrentAppearance } from "./appearance";

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

export async function changeAppearance(target: AppearanceTarget, id: string, action: "apply" | "undo" | "use-theme-wallpaper"): Promise<AppearanceProposal> {
  const receipt = await daemonTransportForPeer(target.peer).json<AppearanceProposal>("POST", `${path(target, id)}/${action}`);
  window.dispatchEvent(new CustomEvent(APPEARANCE_CHANGED, { detail: { target, receipt } }));
  return receipt;
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
