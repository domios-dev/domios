import type { PresenceOverlay } from "./presenceOverlays";

// Connector + avatar are one live-gated unit — never emit a pin without a lead agent.
export function isLivePresencePin(overlay: PresenceOverlay): boolean {
  const lead = overlay.agents[0];
  return !!lead?.id && !lead.isPeer;
}

export function livePresencePins(overlays: readonly PresenceOverlay[]): PresenceOverlay[] {
  return overlays.filter(isLivePresencePin);
}
