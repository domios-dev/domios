import { buildHash } from "./buildStamp";
import { baseDaemonTransport, FRONTEND_BOOT_KEY } from "./daemonBootstrap";
import { frontendApiCompatible, frontendApiContract } from "./frontendApiContract";
import { IS_WEB } from "./webApi";
import { reloadInterface } from "./serviceWorkerRegistration";
import {
  clearDriftRecord, decideBuildDrift, isSignedReleasePage, persistDriftRecord, readDriftRecord,
  WEB_BOOT_CHANGED_EVENT, WEB_EVENTS_READY_EVENT,
  type BootChangedDetail, type DaemonIdentity, type DriftRecord,
} from "./buildDriftReload";

export interface BuildMismatch {
  frontendHash: string;
  backendHash: string;
  frontendVersion: string;
  backendVersion: string;
  reason: "api" | "build" | "build-unreachable";
  apiVersion: number | null;
  minApiVersion: number;
  maxApiVersion: number;
}

export interface BuildDriftEnv {
  isWeb(): boolean;
  storage(): Pick<Storage, "getItem" | "setItem" | "removeItem">;
  origin(): string;
  visible(): boolean;
  now(): number;
  reload(build?: string): void;
}

type MismatchListener = (m: BuildMismatch | null) => void;

let _mismatch: BuildMismatch | null = null;
let _proven = false;
let _generation = 0;
let _deferred: { generation: number; identity: DaemonIdentity; backend: BackendIdentity | null } | null = null;
let _listeners: MismatchListener[] = [];
let _frontendHashOverride: string | null = null;
let _frontendVersionOverride: string | null = null;
let _envOverride: BuildDriftEnv | null = null;

const browserEnv: BuildDriftEnv = {
  isWeb: () => IS_WEB,
  storage: () => sessionStorage,
  origin: () => baseDaemonTransport().origin(),
  visible: () => typeof document === "undefined" || document.visibilityState !== "hidden",
  now: () => Date.now(),
  reload: (build = "current") => {
    void reloadInterface(build)
      .catch(() => { void recheckBuildDrift(); });
  },
};

function env(): BuildDriftEnv {
  return _envOverride ?? browserEnv;
}

function getFrontendHash(): string {
  if (_frontendHashOverride !== null) return _frontendHashOverride;
  return buildHash();
}

function getFrontendVersion(): string {
  if (_frontendVersionOverride !== null) return _frontendVersionOverride;
  return frontendApiContract.version;
}

export async function checkBuildMismatch(apiBase?: string): Promise<BuildMismatch | null> {
  if (_proven) return _mismatch;
  return runCheck(apiBase, undefined);
}

/** A trigger after bootstrap: a local boot-changed frame (optional build hint) or a stream reconnect. */
export function recheckBuildDrift(hint?: string): Promise<BuildMismatch | null> {
  return runCheck(undefined, hint);
}

async function runCheck(apiBase: string | undefined, hint: string | undefined): Promise<BuildMismatch | null> {
  const generation = ++_generation;
  _deferred = null;
  const backend = hint === undefined ? await fetchBackendIdentity(apiBase) : null;
  if (generation !== _generation) return _mismatch;
  const identity: DaemonIdentity = hint !== undefined ? { kind: "hint", build: hint }
    : backend ? { kind: "health", build: backend.build ?? null, apiCompatible: frontendApiCompatible(backend.apiVersion) }
    : { kind: "unreachable" };
  if (identity.kind === "health") _proven = true;
  apply(generation, identity, backend);
  return _mismatch;
}

function safeStorage(): Pick<Storage, "getItem" | "setItem" | "removeItem"> | null {
  try { return env().storage(); } catch { return null; }
}

function apply(generation: number, identity: DaemonIdentity, backend: BackendIdentity | null): void {
  const e = env();
  const storage = safeStorage();
  let origin = "";
  try { origin = e.origin(); } catch { /* no bootstrap: records never match */ }
  const decision = decideBuildDrift({
    isWeb: e.isWeb(),
    releasePage: storage ? isSignedReleasePage(storage, FRONTEND_BOOT_KEY) : false,
    relayedTrigger: false,
    loadedBuild: getFrontendHash(),
    origin,
    daemon: identity,
    visible: e.visible(),
    record: storage ? readDriftRecord(storage) : null,
    now: e.now(),
  });
  switch (decision.action) {
    case "banner": publish(mismatchFor(decision.reason, backend, identity)); return;
    case "clear":
      if (storage) clearDriftRecord(storage);
      if (_mismatch && _mismatch.reason !== "api") publish(null);
      return;
    case "defer": _deferred = { generation, identity, backend }; return;
    case "reload": reloadOnce(decision.record, storage, backend, identity); return;
    case "none": return;
  }
}

function reloadOnce(record: DriftRecord, storage: Pick<Storage, "getItem" | "setItem"> | null, backend: BackendIdentity | null, identity: DaemonIdentity): void {
  if (!storage || !persistDriftRecord(storage, record)) {
    publish(mismatchFor("build", backend, identity));
    return;
  }
  env().reload(record.build);
}

function mismatchFor(reason: BuildMismatch["reason"], backend: BackendIdentity | null, identity: DaemonIdentity): BuildMismatch {
  const backendHash = backend?.build ?? (identity.kind === "hint" ? identity.build : null);
  return {
    frontendHash: getFrontendHash(),
    backendHash: backendHash ?? "unknown",
    frontendVersion: getFrontendVersion(),
    backendVersion: backend?.version ?? "unknown",
    reason,
    apiVersion: typeof backend?.apiVersion === "number" ? backend.apiVersion : null,
    minApiVersion: frontendApiContract.minApiVersion,
    maxApiVersion: frontendApiContract.maxApiVersion,
  };
}

function publish(next: BuildMismatch | null): void {
  _mismatch = next;
  for (const fn of _listeners) fn(_mismatch);
}

/** Re-decides a deferred drift once the page is visible again, without a second fetch. */
export function resumeDeferredBuildDrift(): void {
  const deferred = _deferred;
  if (!deferred || deferred.generation !== _generation || !env().visible()) return;
  _deferred = null;
  apply(deferred.generation, deferred.identity, deferred.backend);
}

/** Wires the post-bootstrap triggers; a relayed boot-changed frame names a peer and is ignored. */
export function installBuildDriftTriggers(target: Pick<Window, "addEventListener" | "removeEventListener"> = window,
  doc: Pick<Document, "addEventListener" | "removeEventListener"> = document): () => void {
  const onBootChanged = (event: Event) => {
    const detail = (event as CustomEvent<BootChangedDetail>).detail;
    if (!detail || detail.relayed) return;
    void recheckBuildDrift(typeof detail.build === "string" ? detail.build : undefined);
  };
  const onReady = () => { void recheckBuildDrift(); };
  const onVisibility = () => resumeDeferredBuildDrift();
  target.addEventListener(WEB_BOOT_CHANGED_EVENT, onBootChanged);
  target.addEventListener(WEB_EVENTS_READY_EVENT, onReady);
  doc.addEventListener("visibilitychange", onVisibility);
  return () => {
    target.removeEventListener(WEB_BOOT_CHANGED_EVENT, onBootChanged);
    target.removeEventListener(WEB_EVENTS_READY_EVENT, onReady);
    doc.removeEventListener("visibilitychange", onVisibility);
  };
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
  _proven = false;
  _generation = 0;
  _deferred = null;
  _listeners = [];
  _frontendHashOverride = null;
  _frontendVersionOverride = null;
  _envOverride = null;
}

/** Test-only: override the frontend hash constant (Vite doesn't run in tsx). */
export function __setFrontendHashForTest(hash: string | null): void {
  _frontendHashOverride = hash;
}

/** Test-only: override the frontend version constant (Vite doesn't run in tsx). */
export function __setFrontendVersionForTest(version: string | null): void {
  _frontendVersionOverride = version;
}

/** Test-only: inject platform, storage, origin, visibility, clock and reload. */
export function __setBuildDriftEnvForTest(next: BuildDriftEnv | null): void {
  _envOverride = next;
}
