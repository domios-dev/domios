/** Generate a short unique ID (good enough for client-side use). */
export function uid(): string {
  return crypto.randomUUID();
}
