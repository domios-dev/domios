const STORAGE_KEY = "codeterm.sidebar.workspace-card-initials";
const COLOR_STORAGE_KEY = "codeterm.sidebar.workspace-card-colors";
const SELECTED_WORKSPACE_KEY = "codeterm.sidebar.selected-workspace";

export type WorkspaceInitials = Record<string, string>;
export type WorkspaceColorIndexes = Record<string, number>;

export const WORKSPACE_CARD_COLORS = [
  "var(--ct-blue)",
  "var(--ct-magenta)",
  "var(--ct-cyan)",
  "var(--ct-orange)",
  "var(--ct-green)",
  "var(--ct-yellow)",
] as const;

export const WORKSPACE_CARD_COLOR_NAMES = ["Blue", "Magenta", "Cyan", "Orange", "Green", "Yellow"] as const;

export function normalizeWorkspaceInitials(value: string): string {
  return Array.from(value.trim().toLocaleUpperCase()).filter((char) => /[\p{L}\p{N}]/u.test(char)).slice(0, 2).join("");
}

export function defaultWorkspaceInitials(name: string): string {
  const words = name
    .replace(/([\p{Ll}\p{N}])(\p{Lu})/gu, "$1 $2")
    .split(/[\s_-]+/u)
    .map((part) => normalizeWorkspaceInitials(part))
    .filter(Boolean);
  if (words.length > 1) return `${words[0][0]}${words[1][0]}`;
  return normalizeWorkspaceInitials(words[0] ?? name) || "WS";
}

export function workspaceCardColorIndex(workspaceId: string): number {
  let hash = 0;
  for (const char of workspaceId) hash = ((hash << 5) - hash + char.codePointAt(0)!) | 0;
  return Math.abs(hash) % WORKSPACE_CARD_COLORS.length;
}

export function workspaceCardColor(workspaceId: string, colorIndex?: number): string {
  return WORKSPACE_CARD_COLORS[colorIndex ?? workspaceCardColorIndex(workspaceId)];
}

export function loadWorkspaceInitials(): WorkspaceInitials {
  try {
    const value = JSON.parse(window.localStorage.getItem(STORAGE_KEY) ?? "{}") as Record<string, unknown>;
    return Object.fromEntries(Object.entries(value).flatMap(([id, initials]) => {
      const normalized = typeof initials === "string" ? normalizeWorkspaceInitials(initials) : "";
      return normalized ? [[id, normalized]] : [];
    }));
  } catch {
    return {};
  }
}

export function persistWorkspaceInitials(initials: WorkspaceInitials): void {
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(initials));
  } catch {}
}

export function loadWorkspaceColorIndexes(): WorkspaceColorIndexes {
  try {
    const value = JSON.parse(window.localStorage.getItem(COLOR_STORAGE_KEY) ?? "{}") as Record<string, unknown>;
    return Object.fromEntries(Object.entries(value).flatMap(([id, index]) => (
      typeof index === "number" && Number.isInteger(index) && index >= 0 && index < WORKSPACE_CARD_COLORS.length
        ? [[id, index]]
        : []
    )));
  } catch {
    return {};
  }
}

export function persistWorkspaceColorIndexes(colors: WorkspaceColorIndexes): void {
  try {
    window.localStorage.setItem(COLOR_STORAGE_KEY, JSON.stringify(colors));
  } catch {}
}

export function loadSelectedWorkspaceId(): string | null {
  try {
    return window.localStorage.getItem(SELECTED_WORKSPACE_KEY);
  } catch {
    return null;
  }
}

export function persistSelectedWorkspaceId(workspaceId: string): void {
  try {
    window.localStorage.setItem(SELECTED_WORKSPACE_KEY, workspaceId);
  } catch {}
}
