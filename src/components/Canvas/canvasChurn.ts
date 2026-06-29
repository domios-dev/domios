export interface CanvasChurnSnapshot {
  canvasCommits: number;
  sceneBuilds: number;
  sceneReadbackPublishes: number;
  renderReadbackBuilds: number;
  presenceSourceBuilds: number;
  presenceOverlayBuilds: number;
  swarmRosterBuilds: number;
  canvasBlits: number;
  canvasResharpens: number;
}

type CanvasChurnKey = keyof CanvasChurnSnapshot;

const EMPTY_CHURN: CanvasChurnSnapshot = {
  canvasCommits: 0,
  sceneBuilds: 0,
  sceneReadbackPublishes: 0,
  renderReadbackBuilds: 0,
  presenceSourceBuilds: 0,
  presenceOverlayBuilds: 0,
  swarmRosterBuilds: 0,
  canvasBlits: 0,
  canvasResharpens: 0,
};

interface CanvasChurnApi {
  reset: () => void;
  snapshot: () => CanvasChurnSnapshot;
}

declare global {
  interface Window {
    __codetermCanvasChurn?: CanvasChurnSnapshot & CanvasChurnApi;
  }
}

function ensureChurn(): (CanvasChurnSnapshot & CanvasChurnApi) | null {
  if (typeof window === "undefined") return null;
  const existing = window.__codetermCanvasChurn;
  if (existing) return existing;
  const counters = {
    ...EMPTY_CHURN,
    reset: () => {
      for (const key of Object.keys(EMPTY_CHURN) as CanvasChurnKey[]) counters[key] = 0;
    },
    snapshot: () => {
      const out = {} as CanvasChurnSnapshot;
      for (const key of Object.keys(EMPTY_CHURN) as CanvasChurnKey[]) out[key] = counters[key];
      return out;
    },
  };
  window.__codetermCanvasChurn = counters;
  return counters;
}

ensureChurn();

export function bumpCanvasChurn(key: CanvasChurnKey, by = 1): void {
  const counters = ensureChurn();
  if (counters) counters[key] += by;
}
