import path from "node:path";

// Mirrors dev-console REPO_ROOT without importing it (would cycle).
export function repoRoot(): string {
  return process.env.CT_E2E_DEV_ROOT
    ? path.resolve(process.env.CT_E2E_DEV_ROOT)
    : path.resolve(__dirname, "..", "..", "..");
}

// A replay.mjs token: matching-quote-wrapped, or unquoted (no leading quote).
const REPLAY_TOKEN = /(^|\s)('[^']*replay\.mjs'|"[^"]*replay\.mjs"|[^\s'"][^\s]*replay\.mjs|replay\.mjs)(\s|$)/;

function unquote(token: string): string {
  const q = token[0];
  return (q === "'" || q === '"') && token.endsWith(q) ? token.slice(1, -1) : token;
}

function isAbsoluteReplay(bare: string): boolean {
  return path.isAbsolute(bare) || /^[A-Za-z]:[\\/]/.test(bare) || bare.startsWith("\\\\");
}

function quoteIfNeeded(p: string): string {
  return /\s/.test(p) ? `'${p}'` : p;
}

function resolveUnderRoot(root: string, relative: string): string {
  if (root.startsWith("/")) return path.posix.resolve(root, relative);
  if (/^[A-Za-z]:[\\/]/.test(root) || root.startsWith("\\\\")) {
    return path.win32.resolve(root, relative);
  }
  return path.resolve(root, relative);
}

// Resolve a repo-relative replay.mjs token to an absolute path; else byte-stable.
export function normalizeStubAgentCmd(cmd: string | undefined, root: string): string | undefined {
  if (cmd === undefined) return cmd;
  const match = REPLAY_TOKEN.exec(cmd);
  if (!match) return cmd;
  const bare = unquote(match[2]);
  if (isAbsoluteReplay(bare)) return cmd;
  const absolute = quoteIfNeeded(resolveUnderRoot(root, bare));
  return cmd.slice(0, match.index) + match[1] + absolute + match[3] + cmd.slice(match.index + match[0].length);
}

export type DevInstance = {
  slot: number;
  name: string;
  apiPort: number;
  vitePort: number;
  cdpPort: number;
  identifier: string;
  dataDirName: string;
  apiBase: string;
  cdpBase: string;
  webUrl: string;
};

function integerEnv(name: string, fallback: number): number {
  const raw = process.env[name];
  const value = raw === undefined ? fallback : Number(raw);
  if (!Number.isInteger(value) || value < 1) {
    throw new Error(`${name} must be a positive integer, got ${raw}`);
  }
  return value;
}

function slugOffset(slug: string): number {
  return (Buffer.from(slug).reduce((sum, byte) => sum + byte, 0) % 200) + 1;
}

export function playwrightWorkerCount(): number {
  return integerEnv("CT_E2E_WORKERS", 1);
}

export function playwrightCargoTargetDir(
  _instance: DevInstance,
  baseEnv: NodeJS.ProcessEnv = process.env,
): string | undefined {
  return baseEnv.CARGO_TARGET_DIR?.trim() || undefined;
}

export function devStartArgs(): string[] {
  return ["build.sh", "dev", "--wait"];
}

export function buildDevInstanceEnv(
  instance: DevInstance,
  baseEnv: NodeJS.ProcessEnv = process.env,
): NodeJS.ProcessEnv {
  const cargoTargetDir = playwrightCargoTargetDir(instance, baseEnv);
  const { CARGO_TARGET_DIR: _ignoredCargoTargetDir, ...inheritedEnv } = baseEnv;
  const stubAgentCmd = normalizeStubAgentCmd(baseEnv.CODETERM_STUB_AGENT_CMD, repoRoot());
  return {
    ...inheritedEnv,
    ...(stubAgentCmd === undefined ? {} : { CODETERM_STUB_AGENT_CMD: stubAgentCmd }),
    CODETERM_INSTANCE: instance.name,
    CODETERM_API_PORT: String(instance.apiPort),
    CODETERM_VITE_PORT: String(instance.vitePort),
    CODETERM_CDP_PORT: String(instance.cdpPort),
    CT_E2E_DATADIR: instance.dataDirName,
    CT_E2E_PORT: String(instance.apiPort),
    ...(cargoTargetDir === undefined ? {} : { CARGO_TARGET_DIR: cargoTargetDir }),
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${instance.cdpPort}`,
    VITE_CODETERM_TEST: "1",
    CODETERM_TEST_CLOCK: "1",
  };
}

export function devInstanceTeardownPorts(instance: DevInstance): number[] {
  return [instance.apiPort, instance.vitePort, instance.cdpPort];
}

export interface InstanceLifecycle {
  kill: (instance: DevInstance) => void;
  assertStopped: (instance: DevInstance) => Promise<void>;
}

// Reverse start order; a per-instance kill/assert failure never aborts the
// sweep — every instance is torn down and failures surface together.
export async function teardownInstances(
  instances: DevInstance[],
  lifecycle: InstanceLifecycle,
): Promise<void> {
  const failures: unknown[] = [];
  for (const instance of [...instances].reverse()) {
    try {
      lifecycle.kill(instance);
      await lifecycle.assertStopped(instance);
    } catch (error) {
      failures.push(error);
    }
  }
  if (failures.length > 0) {
    throw new AggregateError(failures, "Playwright instance teardown leaked processes");
  }
}

export function devInstanceForSlot(slot: number, workers = playwrightWorkerCount()): DevInstance {
  if (!Number.isInteger(slot) || slot < 0 || slot >= workers) {
    throw new Error(`Playwright worker slot ${slot} is outside 0..${workers - 1}`);
  }

  if (workers === 1) {
    const name = process.env.CT_E2E_INSTANCE ?? "playwright";
    const apiPort = integerEnv("CT_E2E_PORT", 7786);
    const offset = name ? slugOffset(name) : 0;
    const vitePort = integerEnv("CT_E2E_VITE_PORT", 1420 + offset);
    const cdpPort = integerEnv("CT_E2E_CDP_PORT", 9223 + offset);
    const dataDirName = process.env.CT_E2E_DATADIR ?? (name ? `.codeterm-dev-${name}` : ".codeterm-dev");
    return descriptor(slot, name, apiPort, vitePort, cdpPort, dataDirName);
  }

  const name = `pw${slot + 1}`;
  const apiPort = integerEnv("CT_E2E_PORT_BASE", 7786) + slot;
  const offset = slugOffset(name);
  return descriptor(
    slot,
    name,
    apiPort,
    1420 + offset,
    9223 + offset,
    `.codeterm-dev-${name}`,
  );
}

function descriptor(
  slot: number,
  name: string,
  apiPort: number,
  vitePort: number,
  cdpPort: number,
  dataDirName: string,
): DevInstance {
  for (const port of [apiPort, vitePort, cdpPort]) {
    if (port === 7685 || port === 7686 || port > 65_535) {
      throw new Error(`refusing unsafe Playwright instance port ${port}`);
    }
  }
  if (name && !/^[a-z0-9-]+$/.test(name)) {
    throw new Error(`refusing unsafe Playwright instance name ${name}`);
  }
  if (!/^\.codeterm-dev(?:-[a-z0-9-]+)?$/.test(dataDirName)) {
    throw new Error(`refusing unsafe Playwright data dir ${dataDirName}`);
  }
  const identifier = name ? `com.codeterm.dev.${name.replace(/-/g, ".")}` : "com.codeterm.dev";
  const apiBase = `http://127.0.0.1:${apiPort}`;
  return {
    slot,
    name,
    apiPort,
    vitePort,
    cdpPort,
    identifier,
    dataDirName,
    apiBase,
    cdpBase: `http://127.0.0.1:${cdpPort}`,
    webUrl: `${apiBase}/`,
  };
}

export function currentDevInstance(): DevInstance {
  const workers = playwrightWorkerCount();
  const rawSlot = process.env.TEST_PARALLEL_INDEX ?? "0";
  const slot = Number(rawSlot);
  if (!Number.isInteger(slot)) {
    throw new Error(`TEST_PARALLEL_INDEX must be an integer, got ${rawSlot}`);
  }
  return devInstanceForSlot(slot, workers);
}

export function allDevInstances(): DevInstance[] {
  const workers = playwrightWorkerCount();
  return Array.from({ length: workers }, (_, slot) => devInstanceForSlot(slot, workers));
}
