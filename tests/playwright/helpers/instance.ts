const DEFAULT_INSTANCE = "playwright";
const DEFAULT_API_PORT = "7786";

function instanceSlug(instance: string): string {
  return instance.toLowerCase().replace(/[^a-z0-9-]/g, "-");
}

function portOffset(slug: string): number {
  return (Buffer.from(slug).reduce((sum, byte) => sum + byte, 0) % 200) + 1;
}

export type PlaywrightInstance = {
  name: string;
  slug: string;
  apiPort: string;
  vitePort: number;
  cdpPort: number;
  identifier: string;
  dataDirName: string;
};

export function configurePlaywrightInstance(): PlaywrightInstance {
  const name = (process.env.CT_E2E_INSTANCE ?? DEFAULT_INSTANCE).trim();
  const slug = instanceSlug(name);
  const apiPort = process.env.CT_E2E_PORT ?? DEFAULT_API_PORT;
  const numericPort = Number(apiPort);

  if (!name || !/[a-z0-9]/.test(slug)) {
    throw new Error("CT_E2E_INSTANCE must contain a letter or number");
  }
  if (!Number.isInteger(numericPort) || numericPort < 1 || numericPort > 65535) {
    throw new Error(`invalid CT_E2E_PORT: ${apiPort}`);
  }
  if (numericPort === 7685 || numericPort === 7686) {
    throw new Error(
      `Playwright refuses shared CodeTerm port ${apiPort}; choose a dedicated CT_E2E_PORT`,
    );
  }

  const offset = portOffset(slug);
  if (numericPort === 1420 + offset || numericPort === 9223 + offset) {
    throw new Error(`CT_E2E_PORT ${apiPort} collides with the named instance's UI ports`);
  }
  const instance = {
    name,
    slug,
    apiPort,
    vitePort: 1420 + offset,
    cdpPort: 9223 + offset,
    identifier: `com.codeterm.dev.${slug.replace(/-/g, ".")}`,
    dataDirName: `.codeterm-dev-${slug}`,
  };

  process.env.CODETERM_INSTANCE = instance.name;
  process.env.CODETERM_API_PORT = instance.apiPort;
  process.env.CODETERM_VITE_PORT = String(instance.vitePort);
  process.env.CODETERM_CDP_PORT = String(instance.cdpPort);
  process.env.CODETERM_DEV_IDENTIFIER = instance.identifier;
  process.env.CT_E2E_PORT = instance.apiPort;
  process.env.CT_E2E_DATADIR = instance.dataDirName;
  return instance;
}

export const PLAYWRIGHT_INSTANCE = configurePlaywrightInstance();
