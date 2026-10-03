export const SYSTEM_ORIGIN_LABEL = "System";

export function systemOriginLabel(context?: string): string {
  const label = context?.trim();
  if (!label || label.toLowerCase() === SYSTEM_ORIGIN_LABEL.toLowerCase()) {
    return SYSTEM_ORIGIN_LABEL;
  }
  return `${SYSTEM_ORIGIN_LABEL} · ${label}`;
}
