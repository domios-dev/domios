/**
 * Provider-CLI discovery for @live specs. A hardcoded install path turns a spec
 * into a permanent silent no-op on every other machine: existsSync fails, the
 * test skips, and the run reports success while asserting nothing.
 */
import { existsSync } from "node:fs";
import os from "node:os";
import path from "node:path";

export interface DiscoverOptions {
  /** Env var holding an explicit path. A set-but-wrong value raises. */
  envVar?: string;
  /** Extra directories searched after PATH (per-user installers). */
  extraDirs?: string[];
  env?: NodeJS.ProcessEnv;
  platform?: NodeJS.Platform;
  exists?: (candidate: string) => boolean;
}

function executableSuffixes(platform: NodeJS.Platform, env: NodeJS.ProcessEnv): string[] {
  if (platform !== "win32") return [""];
  const pathext = env.PATHEXT ?? ".COM;.EXE;.BAT;.CMD";
  const suffixes = pathext
    .split(";")
    .map((entry) => entry.trim())
    .filter(Boolean);
  // A suffix-less hit still counts: shims installed without an extension exist.
  return ["", ...suffixes];
}

export function discoverExecutable(name: string, options: DiscoverOptions = {}): string | null {
  const env = options.env ?? process.env;
  const platform = options.platform ?? process.platform;
  const exists = options.exists ?? existsSync;

  const envVar = options.envVar;
  if (envVar) {
    const override = env[envVar]?.trim();
    if (override) {
      if (!exists(override)) {
        throw new Error(`${envVar}=${override} does not exist`);
      }
      return override;
    }
  }

  const suffixes = executableSuffixes(platform, env);
  const separator = platform === "win32" ? ";" : ":";
  const pathDirs = (env.PATH ?? env.Path ?? "").split(separator).filter(Boolean);
  for (const dir of [...pathDirs, ...(options.extraDirs ?? [])]) {
    for (const suffix of suffixes) {
      const candidate = path.join(dir.replace(/^"|"$/g, ""), `${name}${suffix}`);
      if (exists(candidate)) return candidate;
    }
  }
  return null;
}

/** Per-user install dirs cursor-agent's own installer writes to. */
export function cursorAgentDirs(home: string = os.homedir()): string[] {
  return [
    path.join(home, ".local", "bin"),
    path.join(home, ".cursor", "bin"),
    path.join(home, "AppData", "Local", "Programs", "cursor-agent"),
  ];
}

export const CURSOR_AGENT_BIN_ENV = "CT_E2E_CURSOR_AGENT_BIN";

export function discoverCursorAgent(options: DiscoverOptions = {}): string | null {
  const env = options.env ?? process.env;
  const home = env.HOME ?? env.USERPROFILE ?? os.homedir();
  return discoverExecutable("cursor-agent", {
    envVar: CURSOR_AGENT_BIN_ENV,
    ...options,
    extraDirs: [...cursorAgentDirs(home), ...(options.extraDirs ?? [])],
  });
}
