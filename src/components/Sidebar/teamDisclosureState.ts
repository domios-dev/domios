const STORAGE_KEY = "codeterm.sidebar.team-disclosures";

interface TeamDisclosureState {
  expanded?: boolean;
  workersExpanded?: boolean;
}

function readStates(): Record<string, TeamDisclosureState> {
  try {
    const value = JSON.parse(window.localStorage.getItem(STORAGE_KEY) ?? "{}");
    return value && typeof value === "object" ? value : {};
  } catch {
    return {};
  }
}

export function loadTeamDisclosureState(teamId: string): Required<TeamDisclosureState> {
  const state = readStates()[teamId];
  return {
    expanded: state?.expanded !== false,
    workersExpanded: state?.workersExpanded !== false,
  };
}

export function persistTeamDisclosureState(teamId: string, patch: TeamDisclosureState) {
  try {
    const states = readStates();
    states[teamId] = { ...states[teamId], ...patch };
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(states));
  } catch {}
}
