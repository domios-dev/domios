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

// ── host compute base (host.worker / workspace / agent / mem) ──
// Split by cost (Track C2): worker + agent genuinely spawn long-lived agents,
// so they bridge the sync VM to async host work via a job-id/poll model (F4)
// and are typed `Promise` (the VM preamble wraps the sync binds in resolved
// Promises). mem search + the cheap workspace registry ops are SYNCHRONOUS —
// the VM has no event loop mid-call, so a sync stage (e.g. chatPreprocess) can
// call them and use the result directly. They are gated by enforced
// permissions (`manage_workspaces` / `spawn_agents`).

export interface Backend {
  provider?: string;
  model?: string;
}

export interface PaneRef {
  paneId: string;
  title?: string;
  // "Shell" | "Agent" | "ChatBackend" — the pane's backing discriminator (F1).
  backing?: string;
}

// Mirrors the host's existing parseUsage output (tokens / cost), read-only —
// consumed by a plugin to drive its own reminder cadence (R6). Loosely typed
// because the underlying provider shapes vary.
export interface UsageSnapshot {
  inputTokens?: number;
  outputTokens?: number;
  totalTokens?: number;
  costUsd?: number;
  [key: string]: unknown;
}

// One-shot agent compute. start() enqueues and returns immediately; poll() reads
// the GraphWorkerRegistry — the VM is never held for the turn.
export interface HostWorker {
  start(opts: { backend?: Backend; task: string; timeoutMs?: number }): Promise<{ jobId: string }>;
  poll(jobId: string): Promise<{ done: boolean; report?: string; error?: string }>;
}

// Synchronous: workspace ensure/list are cheap mux ops, not agent turns.
export interface HostWorkspace {
  ensure(opts: { name: string; externalRoot?: string }): { workspaceId: string };
  panes(workspaceId: string): PaneRef[];
}

// A team of long-lived agent sessions the PLUGIN owns and reaps (R2). spawn starts
// a FRESH session; resume re-attaches a SPECIFIC stored session; get looks up the
// plugin's own key→session map. send/poll is the job-id/poll bridge (F4) — send
// returns a ticket, the plugin polls for the reply, the VM never blocks.
export interface HostAgent {
  spawn(
    workspaceId: string,
    opts: { backend?: Backend; task: string; key?: string; commands?: string[] },
  ): Promise<{ sessionId: string }>;
  resume(workspaceId: string, sessionId: string): Promise<{ sessionId: string } | null>;
  get(workspaceId: string, key: string): Promise<{ sessionId: string } | null>;
  send(sessionId: string, text: string): Promise<{ ticket: string }>;
  poll(ticket: string): Promise<{ done: boolean; reply?: string }>;
  reap(sessionId: string): Promise<void>;
  usage?(sessionId: string): Promise<UsageSnapshot | null>;
}

// Synchronous: mem search is sync (mem/manager_search.rs), so a sync stage like
// chatPreprocess can call it mid-turn and use the hits directly.
export interface HostMem {
  search(opts: {
    query: string;
    k?: number;
  }): { hits: { id: string; text: string; score: number }[] };
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

  // ── host compute base (permissioned; job-id/poll bridge) ──
  worker: HostWorker;
  workspace: HostWorkspace;
  agent: HostAgent;
  mem: HostMem;
}

declare global {
  // QuickJS injects `host` as a global before the plugin body runs.
  const host: Host;
}
