/**
 * Daemon base resolution for harness components that call BACK into a CodeTerm
 * daemon (mesh pair-accept, announce). dev-instance refuses to LISTEN on the
 * shared 7685/7686 instances; anything that TALKS to a daemon must refuse them
 * too, and must never fall back to a fixed port when resolution fails.
 */
import { currentDevInstance } from "./dev-instance";

export const MESH_DAEMON_BASE_ENV = "CT_E2E_MESH_DAEMON_BASE";

/** Prod (7685) and the interactive dev console (7686) are never harness targets. */
const SHARED_INSTANCE_PORTS = new Set([7685, 7686]);

export function assertHarnessDaemonBase(base: string, source: string): string {
  const raw = String(base ?? "").trim();
  if (!raw) {
    throw new Error(`harness daemon base from ${source} is empty; refusing to guess a port`);
  }
  let url: URL;
  try {
    url = new URL(raw);
  } catch {
    throw new Error(`harness daemon base from ${source} is not an absolute URL: ${raw}`);
  }
  if (url.protocol !== "http:" && url.protocol !== "https:") {
    throw new Error(`harness daemon base from ${source} must be http(s): ${raw}`);
  }
  const port = Number(url.port);
  if (!url.port || !Number.isInteger(port) || port < 1 || port > 65_535) {
    throw new Error(`harness daemon base from ${source} must carry an explicit port: ${raw}`);
  }
  if (SHARED_INSTANCE_PORTS.has(port)) {
    throw new Error(
      `refusing harness daemon base on shared instance port ${port} (${source}); ` +
        `7685 is prod and 7686 is the interactive dev console`,
    );
  }
  return url.origin;
}

export interface DaemonBaseOptions {
  /** Caller-supplied base; when present it is validated, never bypassed. */
  explicit?: string;
  env?: NodeJS.ProcessEnv;
  /** Harness instance base; injected so the resolver stays unit-testable. */
  instanceBase?: () => string;
}

export function resolveHarnessDaemonBase(options: DaemonBaseOptions = {}): string {
  const env = options.env ?? process.env;
  if (options.explicit !== undefined) {
    return assertHarnessDaemonBase(options.explicit, "daemonBase option");
  }
  const override = env[MESH_DAEMON_BASE_ENV];
  if (override !== undefined) {
    return assertHarnessDaemonBase(override, MESH_DAEMON_BASE_ENV);
  }
  const resolve = options.instanceBase ?? (() => currentDevInstance().apiBase);
  let base: string;
  try {
    base = resolve();
  } catch (error) {
    const reason = error instanceof Error ? error.message : String(error);
    throw new Error(
      `harness daemon base is unresolvable from the dev instance (${reason}); ` +
        `set ${MESH_DAEMON_BASE_ENV} instead of relying on a default`,
    );
  }
  return assertHarnessDaemonBase(base, "harness dev instance");
}
