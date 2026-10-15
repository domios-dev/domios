export function shouldRefetch(
  prevVersion: number | undefined,
  nextVersion: number,
  connectionEvent: boolean,
  _lastError: string | null,
): boolean {
  return connectionEvent || prevVersion !== nextVersion;
}

export interface ProviderIconDescriptor {
  id: string;
  display_name: string;
  icon_svg?: string;
  icon_hint?: string;
  icon_color?: string;
}

export function parseProviderIcons(value: string | null): ProviderIconDescriptor[] {
  if (!value) return [];
  const parsed: unknown = JSON.parse(value);
  if (!Array.isArray(parsed)) return [];
  return parsed.filter((item): item is ProviderIconDescriptor =>
    item !== null && typeof item === "object"
    && typeof item.id === "string" && typeof item.display_name === "string"
    && [item.icon_svg, item.icon_hint, item.icon_color].every((field) => field === undefined || typeof field === "string"));
}
