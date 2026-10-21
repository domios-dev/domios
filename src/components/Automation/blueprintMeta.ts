export function blueprintMeta(description: string, state: string | null | undefined, deletesAt: number | null | undefined, now = Date.now()): string {
  if (state !== "stopped" || deletesAt == null) return description;
  const minutes = Math.max(0, Math.ceil((deletesAt - now) / 60_000));
  return [description, `deletes in ${minutes} min`].filter(Boolean).join(" · ");
}
