export function effectiveReadPath(
  inheritedHome: string | null | undefined,
  fallback: string | null,
  subpath: string,
  separator: string,
): string | null {
  if (!inheritedHome) return fallback;
  if (!subpath) return inheritedHome;
  return inheritedHome + (/[\\/]$/.test(inheritedHome) ? "" : separator) + subpath;
}
