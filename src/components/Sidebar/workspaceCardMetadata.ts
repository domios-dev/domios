const STORAGE_KEY = "codeterm.sidebar.workspace-card-initials";
const SELECTED_WORKSPACE_KEY = "codeterm.sidebar.selected-workspace";

export type WorkspaceInitials = Record<string, string>;

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
