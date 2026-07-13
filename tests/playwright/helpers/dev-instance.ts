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

export function devInstanceForSlot(slot: number, workers = playwrightWorkerCount()): DevInstance {
  if (!Number.isInteger(slot) || slot < 0 || slot >= workers) {
    throw new Error(`Playwright worker slot ${slot} is outside 0..${workers - 1}`);
  }

  if (workers === 1) {
    const name = process.env.CT_E2E_INSTANCE ?? "";
    const apiPort = integerEnv("CT_E2E_PORT", 7686);
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
    if (port === 7685 || port > 65_535) {
      throw new Error(`refusing unsafe Playwright instance port ${port}`);
    }
  }
  if (name && !/^[a-z0-9-]+$/.test(name)) {
    throw new Error(`refusing unsafe Playwright instance name ${name}`);
  }
  if (!/^\.codeterm-dev(?:-[a-z0-9-]+)?$/.test(dataDirName)) {
    throw new Error(`refusing unsafe Playwright data dir ${dataDirName}`);
  }
  const identifier = name ? `com.codeterm.dev.${name.replaceAll("-", ".")}` : "com.codeterm.dev";
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
