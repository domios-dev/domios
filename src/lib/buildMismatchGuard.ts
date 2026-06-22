/**
 * Build-hash mismatch guard.
 *
 * Compares the frontend's compile-time stamp (__BUILD_HASH__, injected by Vite)
 * against the backend's /api/health.build. When they differ, the Rust daemon
 * is running a stale build that doesn't know about newly-added commands —
 * the root cause of cryptic "Command X not found" errors in dev.
 */

export interface BuildMismatch {
  frontendHash: string;
  backendHash: string;
}

type MismatchListener = (m: BuildMismatch | null) => void;

let _mismatch: BuildMismatch | null = null;
let _checked = false;
let _listeners: MismatchListener[] = [];
let _frontendHashOverride: string | null = null;

function getFrontendHash(): string {
  if (_frontendHashOverride !== null) return _frontendHashOverride;
  // __BUILD_HASH__ is injected by Vite at build time (see vite.config.ts).
  // In tsx test runs it is not defined, so we fall back to "dev" (no false mismatch).
  try {
    const g = globalThis as unknown as { __BUILD_HASH__?: string };
    return g.__BUILD_HASH__ ?? "dev";
  } catch {
    return "dev";
  }
}

export async function checkBuildMismatch(apiBase?: string): Promise<BuildMismatch | null> {
  if (_checked) return _mismatch;
  _checked = true;

  const frontendHash = getFrontendHash();
  if (!frontendHash || frontendHash === "dev") return null;

  const backendHash = await fetchBackendBuildHash(apiBase);
  if (!backendHash) return null;

  if (backendHash !== frontendHash) {
    _mismatch = { frontendHash, backendHash };
    for (const fn of _listeners) fn(_mismatch);
  }
  return _mismatch;
}

export function getMismatch(): BuildMismatch | null {
  return _mismatch;
}

export function onMismatch(fn: MismatchListener): () => void {
  _listeners.push(fn);
  return () => {
    _listeners = _listeners.filter((l) => l !== fn);
  };
}

async function fetchBackendBuildHash(apiBase?: string): Promise<string | null> {
  try {
    const base = apiBase ?? (typeof window !== "undefined" ? window.location.origin : "");
    const res = await fetch(`${base}/api/health`);
    if (!res.ok) return null;
    const data = await res.json() as { build?: string };
    return data.build ?? null;
  } catch {
    return null;
  }
}

/** Test-only: reset singleton state between test cases. */
export function __resetForTest(): void {
  _mismatch = null;
  _checked = false;
  _listeners = [];
  _frontendHashOverride = null;
}

/** Test-only: override the frontend hash constant (Vite doesn't run in tsx). */
export function __setFrontendHashForTest(hash: string | null): void {
  _frontendHashOverride = hash;
}
