export const NAVIGATION_LIMIT = 16_384;
const ENTRY_LIMIT = 24;

export type NavigationDecoder<T> = (value: unknown) => T | undefined;

export function navigationRecord(raw: string | null): Record<string, unknown> {
  if (!raw || raw.length > NAVIGATION_LIMIT) return {};
  try {
    const value: unknown = JSON.parse(raw);
    return value && typeof value === "object" && !Array.isArray(value)
      ? Object.fromEntries(Object.entries(value).slice(-ENTRY_LIMIT)) : {};
  } catch { return {}; }
}

export function readNavigation<T>(storage: Pick<Storage, "getItem"> | null, scope: string, key: string, decode: NavigationDecoder<T>): T | undefined {
  try { return decode(navigationRecord(storage?.getItem(scope) ?? null)[key]); } catch { return undefined; }
}

export function writeNavigation<T>(storage: Pick<Storage, "getItem" | "setItem"> | null, scope: string, key: string, value: T, decode: NavigationDecoder<T>): void {
  try {
    const entries = navigationRecord(storage?.getItem(scope) ?? null);
    delete entries[key];
    const valid = decode(value);
    if (valid !== undefined) entries[key] = valid;
    const retained = Object.entries(entries).slice(-ENTRY_LIMIT);
    let raw = JSON.stringify(Object.fromEntries(retained));
    while (raw.length > NAVIGATION_LIMIT && retained.length > 1) {
      retained.shift();
      raw = JSON.stringify(Object.fromEntries(retained));
    }
    if (raw.length <= NAVIGATION_LIMIT) storage?.setItem(scope, raw);
  } catch {}
}

export function navigationId(value: unknown): value is string {
  return typeof value === "string" && value.length > 0 && value.length <= 512;
}

export function optionalNavigationId(value: unknown): string | null | undefined {
  return value === null ? null : navigationId(value) ? value : undefined;
}

export function navigationObject(value: unknown): Record<string, unknown> | undefined {
  return value && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : undefined;
}

export function parentWhenMissing<T>(target: T | null, ready: boolean, exists: (target: T) => boolean): T | null {
  return target !== null && ready && !exists(target) ? null : target;
}
