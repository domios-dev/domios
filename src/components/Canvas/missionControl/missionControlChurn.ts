export interface MissionControlChurnSnapshot {
  builds: number;
  snapshots: number;
  activeWork: number;
}

type MissionControlChurnKey = keyof MissionControlChurnSnapshot;

const EMPTY_CHURN: MissionControlChurnSnapshot = {
  builds: 0,
  snapshots: 0,
  activeWork: 0,
};

interface MissionControlChurnApi {
  reset: () => void;
  snapshot: () => MissionControlChurnSnapshot;
}

declare global {
  interface Window {
    __codetermMissionControlChurn?: MissionControlChurnSnapshot & MissionControlChurnApi;
  }
}

function ensureChurn(): (MissionControlChurnSnapshot & MissionControlChurnApi) | null {
  if (typeof window === "undefined") return null;
  const existing = window.__codetermMissionControlChurn;
  if (existing) return existing;
  const counters = {
    ...EMPTY_CHURN,
    reset: () => {
      for (const key of Object.keys(EMPTY_CHURN) as MissionControlChurnKey[]) counters[key] = 0;
    },
    snapshot: () => {
      const out = {} as MissionControlChurnSnapshot;
      for (const key of Object.keys(EMPTY_CHURN) as MissionControlChurnKey[]) out[key] = counters[key];
      return out;
    },
  };
  window.__codetermMissionControlChurn = counters;
  return counters;
}

ensureChurn();

export function bumpMissionControlChurn(key: MissionControlChurnKey, by = 1): void {
  const counters = ensureChurn();
  if (counters) counters[key] += by;
}
