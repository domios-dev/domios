/** The cursor tour (spec L15.4): a route over the app's own controls and the human pace it moves at. */

export type GuideId = "team" | "memory" | "mesh" | "plugins" | "neighbor" | "provider" | "voice";

export interface TourStep {
  /** CSS selector of the real control inside the workspace, resolved when the step starts. */
  target: string;
  /** What the step shows, for the audit sheet and tests. */
  note: string;
  /** Extra time to look at the result before the next hop, in ms. */
  look?: number;
  label?: string | readonly string[];
}

const tab = (id: string) => `[data-testid="tab-item"][data-tab-id="${id}"] .sidebar-tab-activation`;

export function tourRoute(ids: { agent: string; home: string; card: string }): TourStep[] {
  return [
    { target: tab(ids.agent), note: "open an agent's chat", look: 3800 },
    { target: '[data-testid="peer-picker-button"]', note: "open the machine picker", look: 400 },
    { target: '[data-testid="peer-picker-item"][data-peer-name="gpu-rig"]', note: "switch to gpu-rig", look: 2800 },
    { target: '[data-testid="peer-picker-button"]', note: "open the machine picker", look: 400 },
    { target: '[data-testid="peer-picker-self"]', note: "back to this computer", look: 600 },
    { target: '[data-testid="team-card-activation"]', note: "open the team's plan", look: 1400 },
    { target: `[aria-label^="Open task ${ids.card}"]`, note: "open a card", look: 2200 },
    { target: '[data-testid="titlebar-extensions"]', note: "open Plugins", look: 1800 },
    { target: '[data-testid="nav-memory"]', note: "open Memory", look: 1800 },
    { target: tab(ids.home), note: "back to the Orchestrator", look: 0 },
  ];
}

export function guideRoute(guide: GuideId, selectedTab: string, home: string): TourStep[] {
  const click = (target: string, label?: string | readonly string[]): TourStep => ({ target, label, note: typeof label === "string" ? label : target });
  switch (guide) {
    case "team":
      return [
        click('[data-testid="sidebar-new-session"]'),
        click('[data-testid="picker-mode-orchestration"]'),
        click('[role="dialog"] button', "Start team"),
      ];
    case "memory":
      return [click('[data-testid="nav-memory"]')];
    case "mesh":
      return [click('[data-testid="nav-mesh"]'), click('button', "Connect sample machine")];
    case "plugins":
      return [click('[data-testid="titlebar-extensions"]')];
    case "neighbor":
      return [
        click('[data-testid="nav-neighbors"]'),
        click('[data-testid="demo-add-neighbor"]'),
        click('[role="dialog"] button', "Send sample invite"),
      ];
    case "provider":
      return [
        click('[data-testid="tab-actions-trigger"], [data-testid="tab-item"][data-tab-id="' + (selectedTab || home) + '"] [aria-label="Tab actions"]'),
        click('[role="menuitem"]', "Switch provider…"),
        click('[role="menuitemradio"][aria-checked="false"]'),
        { ...click('[role="dialog"] button', ["Switch provider", "Stop and switch"]), look: 1720 },
      ];
    case "voice":
      return [click('[role="tab"]', "Chat"), { ...click('[data-testid="demo-voice-button"]'), look: 3600 }];
  }
}

export const HOP = [600, 900] as const;
export const DWELL = [400, 700] as const;
export const PRESS_MS = 105;
export const RING_MS = 300;
export const LOOP_HOLD_MS = 4000;
export const STOP_FADE_MS = 150;
export const RESUME_AFTER_MS = 45_000;

/** A hop's duration grows with distance, within the human range. */
export function hopDuration(dx: number, dy: number): number {
  const distance = Math.hypot(dx, dy);
  return Math.round(HOP[0] + (HOP[1] - HOP[0]) * Math.min(1, distance / 900));
}

/** The pause before a click: varied by step so the loop does not tick like a metronome. */
export function dwellBefore(index: number): number {
  return DWELL[0] + ((index * 137) % (DWELL[1] - DWELL[0] + 1));
}

/** One loop's length in ms with the given hops, from the step timings alone. */
export function loopLength(steps: TourStep[], hops: number[]): number {
  return steps.reduce((sum, step, index) => sum + (hops[index] ?? HOP[1]) + dwellBefore(index) + PRESS_MS + (step.look ?? 0), 0) + LOOP_HOLD_MS;
}
