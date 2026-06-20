// The `host` global injected into every plugin QuickJS VM — the raw host_api.rs
// bindings plus the vm.rs preamble convenience layer. QuickJS is not a browser:
// no console, fetch, timers, or DOM.

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

export interface ExecStartResult {
  jobId?: string;
  error?: string;
}

export interface ExecPollResult {
  done: boolean;
  code?: number;
  stdout?: string;
  stderr?: string;
  error?: string;
}

export interface HostExec {
  (optsJson: string): string;
  // Async exec (start + poll), so the VM lock is held only for the sub-ms calls, not the command's duration.
  start(opts: ExecOpts): ExecStartResult;
  poll(jobId: string): ExecPollResult;
  close(jobId: string): void;
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

export interface FetchStreamStart {
  jobId?: string;
  error?: string;
}

export interface FetchStreamPoll {
  chunks: string[];
  done: boolean;
  status?: number;
  error?: string;
}

export interface HostFetch {
  (optsJson: string): string;
  stream(opts: FetchOpts): FetchStreamStart;
  streamPoll(jobId: string): FetchStreamPoll;
  streamClose(jobId: string): void;
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
// All synchronous: worker/agent use a job-id/poll model that's sync per step (each
// call returns immediately; the long work happens on the host between calls). Not
// Promise-wrapped — QuickJS can't drain a Promise inside a sync `call_json` dispatch.
// agent/workspace are gated by the `manage_workspaces` / `spawn_agents` grants.

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

// Read-only mirror of the host's parseUsage output (tokens/cost); loosely typed since provider shapes vary.
export interface UsageSnapshot {
  inputTokens?: number;
  outputTokens?: number;
  totalTokens?: number;
  costUsd?: number;
  [key: string]: unknown;
}

// One-shot agent compute: start() enqueues and returns immediately, poll() reads the result. Sync per step.
export interface HostWorker {
  start(opts: { backend?: Backend; task: string }): { jobId: string };
  poll(jobId: string): { done: boolean; report?: string; error?: string };
}

// Synchronous: workspace ensure/list are cheap mux ops, not agent turns.
export interface HostWorkspace {
  ensure(opts: { name: string; externalRoot?: string }): { workspaceId: string };
  panes(workspaceId: string): PaneRef[];
}

// Long-lived agent sessions the plugin owns and reaps: spawn (fresh), resume (a stored
// session), get (the plugin's key→session map). send/poll is the job-id bridge — the VM never blocks.
export interface HostAgent {
  spawn(
    workspaceId: string,
    opts: { backend?: Backend; task: string; key?: string; commands?: string[] },
  ): { sessionId: string };
  resume(workspaceId: string, sessionId: string): { sessionId: string } | null;
  get(workspaceId: string, key: string): { sessionId: string } | null;
  send(sessionId: string, text: string): { ticket: string };
  poll(ticket: string): { done: boolean; reply?: string; error?: string };
  reap(sessionId: string): void;
  usage?(sessionId: string): UsageSnapshot | null;
}

// Synchronous: mem search is sync, so a sync stage like chatPreprocess can use the hits directly mid-turn.
export interface HostMem {
  search(opts: {
    query: string;
    k?: number;
  }): { hits: { id: string; text: string; score: number }[] };
}

export interface HostProgressOpts {
  label: string;
  percent?: number;
  done?: boolean;
  error?: string;
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
  exec: HostExec;
  execStart(optsJson: string): string;
  execPoll(jobId: string): string;
  execClose(jobId: string): void;
  fetch: HostFetch;
  fetchStream(optsJson: string): string;
  fetchStreamPoll(jobId: string): string;
  fetchStreamClose(jobId: string): void;
  secretGet(name: string): string | null;
  secretSet(name: string, value: string): boolean;
  secretDelete(name: string): boolean;
  // Tolerant tool-call extractor returning a tri-state JSON `status` so "no call"
  // (`none`) is never confused with "broken call" (`malformed`, must be retried) vs `ok`.
  // `schemaJson` describes the plugin's tools; repairs are re-validated, degrading to malformed/none rather than guessing.
  toolcallParse(rawText: string, schemaJson: string): string;

  // ── Manifest (raw + parsed via preamble) ──
  manifestJson(): string | null;
  manifest(): unknown;

  // ── Preamble convenience layer ──
  fs: HostFs;
  shell: HostShell;
  path: HostPath;
  // Preamble convenience: host.toolcall.parse -> host.toolcallParse.
  toolcall: { parse(rawText: string, schemaJson: string): string };
  progress(opts: HostProgressOpts): void;

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
