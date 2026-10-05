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

function playwrightTarget(): "isolated" | "dev" | "named" {
  const target = process.env.PW_TARGET;
  if (target === undefined || target === "isolated") return "isolated";
  if (target === "dev" || target === "named") return target;
  throw new Error(`PW_TARGET must be absent, isolated, dev, or named; got ${target}`);
}

export function isDevTarget(): boolean {
  return playwrightTarget() === "dev";
}

export function isNamedTarget(): boolean {
  return playwrightTarget() === "named";
}

export function isAdoptTarget(): boolean {
  return playwrightTarget() !== "isolated";
}

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
  if (isAdoptTarget()) {
    const requested = process.env.CT_E2E_WORKERS;
    if (requested !== undefined && Number(requested) !== 1) {
      throw new Error(`PW_TARGET=${playwrightTarget()} requires CT_E2E_WORKERS=1 so one test owns the live UI`);
    }
    return 1;
  }
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

// An AggregateError prints only its summary, so every sub-error is inlined.
export function teardownFailureMessage(failures: unknown[]): string {
  const described = failures.map(
    (failure, index) => `  ${index + 1}. ${failure instanceof Error ? failure.message : String(failure)}`,
  );
  return [`Playwright instance teardown failed (${failures.length}):`, ...described].join("\n");
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
    } catch (error) {
      failures.push(error);
    }
    try {
      await lifecycle.assertStopped(instance);
    } catch (error) {
      failures.push(error);
    }
  }
  if (failures.length > 0) {
    throw new AggregateError(failures, teardownFailureMessage(failures));
  }
}

export function devInstanceForSlot(slot: number, workers = playwrightWorkerCount()): DevInstance {
  if (isDevTarget()) {
    if (slot !== 0 || workers !== 1) {
      throw new Error("PW_TARGET=dev supports exactly one Playwright worker");
    }
    return descriptor(slot, "", 7686, 1420, 9223, ".codeterm-dev", true);
  }
  if (isNamedTarget()) {
    if (slot !== 0 || workers !== 1) {
      throw new Error("PW_TARGET=named supports exactly one Playwright worker");
    }
    const name = process.env.CODETERM_INSTANCE?.trim() ?? "";
    if (!/^[a-z0-9-]+$/.test(name)) {
      throw new Error("PW_TARGET=named requires a resolved lowercase CODETERM_INSTANCE");
    }
    if (!process.env.CODETERM_API_TOKEN?.trim()) {
      throw new Error("PW_TARGET=named requires CODETERM_API_TOKEN from the bench lib resolution");
    }
    const apiPort = integerEnv("CODETERM_API_PORT", 0);
    const dataDir = process.env.CODETERM_DEV_DATA_DIR?.trim();
    const dataDirName = dataDir ? path.basename(dataDir) : "";
    const expectedDataDir = `.codeterm-dev-${name}`;
    if (dataDirName !== expectedDataDir) {
      throw new Error(`PW_TARGET=named requires CODETERM_DEV_DATA_DIR for ${expectedDataDir}`);
    }
    const expectedSocket = `codeterm-dev-${name}.sock`;
    const socketName = (process.env.CODETERM_SOCKET_PATH ?? "").split(/[\\/]/).at(-1);
    if (socketName !== expectedSocket) {
      throw new Error(`PW_TARGET=named requires the resolved socket ${expectedSocket}`);
    }
    const offset = slugOffset(name);
    const vitePort = integerEnv("CODETERM_VITE_PORT", 1420 + offset);
    if (vitePort !== 1420 + offset) {
      throw new Error(`PW_TARGET=named resolved Vite port ${vitePort} does not match instance ${name}`);
    }
    const cdpPort = integerEnv("CODETERM_CDP_PORT", 9223 + offset);
    if (cdpPort !== 9223 + offset) {
      throw new Error(`PW_TARGET=named resolved CDP port ${cdpPort} does not match instance ${name}`);
    }
    return descriptor(slot, name, apiPort, vitePort, cdpPort, dataDirName);
  }
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

export function isDisposableQaInstance(instance: DevInstance): boolean {
  return (
    instance.dataDirName === `.codeterm-dev-${instance.name}` &&
    (instance.name === "playwright" || instance.name === "pwserial" || /^pw[1-9]\d*$/.test(instance.name))
  );
}

function descriptor(
  slot: number,
  name: string,
  apiPort: number,
  vitePort: number,
  cdpPort: number,
  dataDirName: string,
  allowDefaultDev = false,
): DevInstance {
  for (const port of [apiPort, vitePort, cdpPort]) {
    if (port === 7685 || (port === 7686 && !allowDefaultDev) || port > 65_535) {
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
