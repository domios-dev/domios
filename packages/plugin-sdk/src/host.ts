// The `host` global injected into every plugin QuickJS VM — the raw host_api.rs
// bindings plus the vm.rs preamble convenience layer. QuickJS is not a browser:

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
  // Run `opts` off the VM; the host resumes with `then` or refuses when the
  // completion event misses its bound. Must be `return`ed from the plugin method.
  async<T>(opts: ExecOpts, then: (result: ExecPollResult) => T): T;
}

export interface FetchOpts {
  url: string;
  method?: string;
  headers?: Record<string, string>;
  body?: string;
  timeoutMs?: number;
  // Ask the host to stamp a manifest-declared credential into request headers.
  // Templates are filled from that credential's fields (`{accessToken}`); the
  credential?: CredentialRequest;
}

// `plugin.json` -> `credentials[]` declares the file and its field paths; the
// plugin only ever names the credential and the header shape it needs.
export interface CredentialRequest {
  id: string;
  headers: Record<string, string>;
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
  // Bounded reads — up to maxBytes from the head/tail, trimmed to a valid UTF-8 boundary. Read-only.
  readFileHead(path: string, maxBytes: number): string | null;
  readFileTail(path: string, maxBytes: number): string | null;
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
  // Cross-platform "set env var for the next command" prefix — POSIX uses
  // inline assignment (no `&&`/`;` chaining needed), Windows uses `$env:`.
  envPrefix(name: string, value: string, platform: string): string;
}

export interface HostPath {
  isWindows: boolean;
  toForwardSlash(p: string): string;
  toNative(p: string): string;
  normalize(p: string): string;
  equal(a: string, b: string): boolean;
}

// ── host compute base (host.worker / workspace / agent / mem) ──

export interface Backend {
  provider?: string;
  model?: string;
}

export interface TabRef {
  tabId: string;
  title?: string;
  // "Shell" | "Agent" | "ChatBackend" — the tab's backing discriminator (F1).
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
  tabs(workspaceId: string): TabRef[];
}

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
  platform(): string;
  homeDir(): string | null;
  expandHome(path: string): string | null;
  envGet(name: string): string | null;
  // Non-secret fields of a manifest-declared credential, as a JSON object.
  // Secret fields are never returned — see `FetchOpts.credential`.
  credentialPublic(id: string): string | null;
  unixNow(): number;
  unixNowMs(): number;
  md5(s: string): string;

  fileExists(path: string): boolean;
  readFile(path: string): string | null;
  readFileHead(path: string, maxBytes: number): string | null;
  readFileTail(path: string, maxBytes: number): string | null;
  writeFile(path: string, contents: string): boolean;
  removeFile(path: string): boolean;
  makeDirs(path: string): boolean;
  readDirJson(path: string): string | null;
  walkDirJson(path: string, maxDepth: number): string | null;
  writeTempFile(contents: string, suffix?: string): string | null;
  sqliteQueryAllJson(db: string, sql: string, paramsJson?: string | null): string | null;

  shellQuote(s: string): string;
  shellQuoteFor(s: string, platform: string): string;
  shellEnvPrefix(name: string, value: string, platform: string): string;

  log(level: "trace" | "debug" | "info" | "warn" | "error", msg: string): void;

  settingsJson(): string;
  exec: HostExec;
  execStart(optsJson: string): string;
  execPoll(jobId: string): string;
  execClose(jobId: string): void;
  // Yield the VM until `jobId` finishes, then continue with `then`. The host
  // refuses the call when the completion event misses its bound. Only valid as
  awaitJob<T>(jobId: string, then: (result: ExecPollResult) => T): T;
  fetch: HostFetch;
  fetchStream(optsJson: string): string;
  fetchStreamPoll(jobId: string): string;
  fetchStreamClose(jobId: string): void;
  secretGet(name: string): string | null;
  secretSet(name: string, value: string): boolean;
  secretDelete(name: string): boolean;
  // Tolerant tool-call extractor returning a tri-state JSON `status` so "no call"
  // (`none`) is never confused with "broken call" (`malformed`, must be retried) vs `ok`.
  toolcallParse(rawText: string, schemaJson: string): string;

  manifestJson(): string | null;
  manifest(): unknown;

  fs: HostFs;
  shell: HostShell;
  path: HostPath;
  // Preamble convenience: host.toolcall.parse -> host.toolcallParse.
  toolcall: { parse(rawText: string, schemaJson: string): string };
  progress(opts: HostProgressOpts): void;

  worker: HostWorker;
  workspace: HostWorkspace;
  agent: HostAgent;
  mem: HostMem;
}

declare global {
  // QuickJS injects `host` as a global before the plugin body runs.
  const host: Host;
}
