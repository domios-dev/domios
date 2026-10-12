import { buildHash } from "./buildStamp";
import { baseDaemonTransport } from "./daemonBootstrap";
import { frontendApiCompatible, frontendApiContract } from "./frontendApiContract";

export interface BuildMismatch {
  frontendHash: string;
  backendHash: string;
  frontendVersion: string;
  backendVersion: string;
  reason: "api";
  apiVersion: number | null;
  minApiVersion: number;
  maxApiVersion: number;
}

type MismatchListener = (m: BuildMismatch | null) => void;

let _mismatch: BuildMismatch | null = null;
let _checked = false;
let _listeners: MismatchListener[] = [];
let _frontendHashOverride: string | null = null;
let _frontendVersionOverride: string | null = null;

function getFrontendHash(): string {
  if (_frontendHashOverride !== null) return _frontendHashOverride;
  return buildHash();
}

function getFrontendVersion(): string {
  if (_frontendVersionOverride !== null) return _frontendVersionOverride;
  return frontendApiContract.version;
}

export async function checkBuildMismatch(apiBase?: string): Promise<BuildMismatch | null> {
  if (_checked) return _mismatch;
  const frontendHash = getFrontendHash();
  const frontendVersion = getFrontendVersion();
  const backendIdentity = await fetchBackendIdentity(apiBase);
  if (!backendIdentity) return null;
  _checked = true;
  const backendHash = backendIdentity.build ?? null;
  const backendVersion = backendIdentity.version ?? null;

  if (!frontendApiCompatible(backendIdentity.apiVersion)) {
    _mismatch = {
      frontendHash,
      backendHash: backendHash ?? "unknown",
      frontendVersion,
      backendVersion: backendVersion ?? "unknown",
      reason: "api",
      apiVersion: typeof backendIdentity.apiVersion === "number" ? backendIdentity.apiVersion : null,
      minApiVersion: frontendApiContract.minApiVersion,
      maxApiVersion: frontendApiContract.maxApiVersion,
    };
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

interface BackendIdentity { build?: string; version?: string; apiVersion?: unknown }
async function fetchBackendIdentity(apiBase?: string): Promise<BackendIdentity | null> {
  try {
    if (apiBase === undefined) return await baseDaemonTransport().localJson<BackendIdentity>("GET", "/api/health");
    const res = await fetch(`${apiBase}/api/health`);
    return res.ok ? await res.json() as BackendIdentity : null;
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
  _frontendVersionOverride = null;
}

/** Test-only: override the frontend hash constant (Vite doesn't run in tsx). */
export function __setFrontendHashForTest(hash: string | null): void {
  _frontendHashOverride = hash;
}

/** Test-only: override the frontend version constant (Vite doesn't run in tsx). */
export function __setFrontendVersionForTest(version: string | null): void {
  _frontendVersionOverride = version;
}
