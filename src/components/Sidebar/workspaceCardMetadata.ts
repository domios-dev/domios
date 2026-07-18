const STORAGE_KEY = "codeterm.sidebar.workspace-card-initials";

export type WorkspaceInitials = Record<string, string>;

export const WORKSPACE_CARD_COLORS = [
  "var(--ct-blue)",
  "var(--ct-magenta)",
  "var(--ct-cyan)",
  "var(--ct-orange)",
  "var(--ct-green)",
  "var(--ct-yellow)",
] as const;

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

export function workspaceCardColor(workspaceId: string): string {
  let hash = 0;
  for (const char of workspaceId) hash = ((hash << 5) - hash + char.codePointAt(0)!) | 0;
  return WORKSPACE_CARD_COLORS[Math.abs(hash) % WORKSPACE_CARD_COLORS.length];
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
