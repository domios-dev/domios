// The `host` global injected into every plugin QuickJS VM.
// Mirrors the real surface installed by src-tauri/src/plugin/js_runtime:
// flat bindings from host_api.rs + the convenience layer added by the
// PLUGIN_PREAMBLE in vm.rs (host.fs / host.shell / host.path / host.manifest).
// QuickJS is not a browser: no console, no fetch, no timers, no DOM.

export interface ExecOpts {
  bin: string;
  args?: string[];
  env?: Record<string, string>;
  stdin?: string;
  timeoutMs?: number;
  detach?: boolean;
  logFile?: string;
}

export interface ExecResult {
  code?: number;
  stdout?: string;
  stderr?: string;
  pid?: number;
  error?: string;
}

export interface FetchOpts {
  url: string;
  method?: string;
  headers?: Record<string, string>;
  body?: string;
  timeoutMs?: number;
}

export interface FetchResult {
  status?: number;
  headers?: Record<string, string>;
  body?: string;
  error?: string;
}

export interface DirEntry {
  name: string;
  path: string;
  isFile: boolean;
  isDir: boolean;
  size: number;
  modifiedMs: number | null;
  createdMs: number | null;
}

// Convenience layer the VM preamble adds on top of the raw JSON bridges.
export interface HostFs {
  readFile(path: string): string | null;
  writeFile(path: string, contents: string): boolean;
  removeFile(path: string): boolean;
  makeDirs(path: string): boolean;
  fileExists(path: string): boolean;
  expandHome(path: string): string | null;
  writeTempFile(contents: string, suffix?: string): string | null;
  readDir(path: string): DirEntry[];
  walkDir(path: string, maxDepth?: number): DirEntry[];
  sqliteQuery<T = Record<string, unknown>>(
    db: string,
    sql: string,
    params?: unknown[],
  ): T[];
  readJson<T = unknown>(path: string): T | null;
  writeJson(path: string, value: unknown): boolean;
}

export interface HostShell {
  quote(s: string): string;
  quoteFor(s: string, platform: string): string;
}

export interface HostPath {
  isWindows: boolean;
  toForwardSlash(p: string): string;
  toNative(p: string): string;
  normalize(p: string): string;
  equal(a: string, b: string): boolean;
}

export interface Host {
  // ── Identity / paths ──
  platform(): string;
  homeDir(): string | null;
  expandHome(path: string): string | null;
  envGet(name: string): string | null;
  unixNow(): number;
  unixNowMs(): number;
  md5(s: string): string;

  // ── Filesystem (raw bindings) ──
  fileExists(path: string): boolean;
  readFile(path: string): string | null;
  writeFile(path: string, contents: string): boolean;
  removeFile(path: string): boolean;
  makeDirs(path: string): boolean;
  readDirJson(path: string): string | null;
  walkDirJson(path: string, maxDepth: number): string | null;
  writeTempFile(contents: string, suffix?: string): string | null;
  sqliteQueryAllJson(db: string, sql: string, paramsJson?: string | null): string | null;

  // ── Shell / process ──
  shellQuote(s: string): string;
  shellQuoteFor(s: string, platform: string): string;

  // ── Logging ──
  log(level: "trace" | "debug" | "info" | "warn" | "error", msg: string): void;

  // ── host_api v2: subprocess / network / secrets / settings ──
  settingsJson(): string;
  exec(optsJson: string): string;
  fetch(optsJson: string): string;
  secretGet(name: string): string | null;
  secretSet(name: string, value: string): boolean;
  secretDelete(name: string): boolean;

  // ── Manifest (raw + parsed via preamble) ──
  manifestJson(): string | null;
  manifest(): unknown;

  // ── Preamble convenience layer ──
  fs: HostFs;
  shell: HostShell;
  path: HostPath;
}

declare global {
  // QuickJS injects `host` as a global before the plugin body runs.
  const host: Host;
}
