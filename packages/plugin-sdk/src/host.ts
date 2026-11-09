import type {OptionalWireFields} from "./wireViews";
// The `host` global injected into every plugin QuickJS VM — the raw host_api.rs
// bindings plus the vm.rs preamble convenience layer. QuickJS is not a browser:

export type ExecConversationStep = import("./gen/ExecConversationStep").ExecConversationStep;

export type ExecOpts = OptionalWireFields<import("./gen/ExecOpts").ExecOpts, "args" | "env" | "stdin" | "conversation" | "timeoutMs" | "detach" | "logFile">;

export type ExecResult = import("./gen/ExecResult").ExecResult;

export type ExecStartResult = Partial<import("./gen/HostJobStart").HostJobStart & import("./gen/HostError").HostError>;

export type ExecPollResult = import("./gen/ExecPollResult").ExecPollResult;

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

export type FetchOpts = Pick<import("./gen/FetchOpts").FetchOpts, "url"> & Partial<Omit<import("./gen/FetchOpts").FetchOpts, "url">> & {credential?: CredentialRequest; profileSecret?: ProfileSecretRequest};

// The credential a launch profile names in its `balance` limits source fills `{secret}`
// header templates host-side; any other variable or profile is refused.
export interface ProfileSecretRequest {
  profile: string;
  env: string;
  headers: Record<string, string>;
}

// `plugin.json` -> `credentials[]` declares the file and its field paths; the
// plugin only ever names the credential and the header shape it needs.
export interface CredentialRequest {
  id: string;
  headers: Record<string, string>;
}

export type FetchResult = import("./gen/FetchResult").FetchResult;

export type FetchStreamStart = Partial<import("./gen/HostJobStart").HostJobStart & import("./gen/HostError").HostError>;

export type FetchStreamPoll = import("./gen/StreamPollResponse").StreamPollResponse;

export interface HostFetch {
  (optsJson: string): string;
  async: <T>(opts: FetchOpts, then: (result: FetchResult) => T) => T;
  stream(opts: FetchOpts): FetchStreamStart;
  streamPoll(jobId: string): FetchStreamPoll;
  streamClose(jobId: string): void;
}

export type DirEntry = import("./gen/DirEntry").DirEntry;

// Convenience layer the VM preamble adds on top of the raw JSON bridges.
export interface HostFs {
  readFile(path: string): string | null;
  // Bounded reads — up to maxBytes from the head/tail, trimmed to a valid UTF-8 boundary. Read-only.
  readFileHead(path: string, maxBytes: number): string | null;
  readFileTail(path: string, maxBytes: number): string | null;
  writeFile(path: string, contents: string): boolean;
  writeFileAtomic(path: string, contents: string): boolean;
  lockFile(path: string): string | null;
  unlockFile(id: string): boolean;
  removeFile(path: string): boolean;
  makeDirs(path: string): boolean;
  fileExists(path: string): boolean;
  isSymlink(path: string): boolean | null;
  /** `~/.domios/...` resolves inside the running instance's data dir. Prefer host.dataDir() for plugin-owned storage. */
  expandHome(path: string): string | null;
  writeTempFile(contents: string, suffix?: string): string | null;
  readDir(path: string): DirEntry[];
  walkDir(path: string, maxDepth?: number): DirEntry[];
  /** Throws RangeError when the result, or any value SQLite reads for it, passes the host's 64 MiB bound. */
  sqliteQuery<T = Record<string, unknown>>(
    db: string,
    sql: string,
    params?: unknown[],
  ): T[];
  /** The same rows as sqliteQuery, read on a host thread; the method must return this call's result. */
  sqliteQueryAsync<T = Record<string, unknown>, R = unknown>(
    db: string,
    sql: string,
    params: unknown[] | null | undefined,
    then: (rows: T[]) => R,
  ): R;
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

export type Backend = Partial<Pick<import("./gen/HostWorkerStartOptions").HostWorkerStartOptions, "provider" | "model">>;

export interface TabRef {
  tabId: string;
  title?: string;
  // "Shell" | "Agent" | "ChatBackend" — the tab's backing discriminator (F1).
  backing?: string;
}

export type UsageSnapshot = import("./gen/ProviderUsageSnapshot").ProviderUsageSnapshot;

// One-shot agent compute: start() enqueues and returns immediately, poll() reads the result. Sync per step.
export interface HostWorker {
  start(opts: OptionalWireFields<import("./gen/HostWorkerStartOptions").HostWorkerStartOptions, "provider" | "model">): Partial<import("./gen/HostJobStart").HostJobStart & import("./gen/HostError").HostError>;
  poll(jobId: string): import("./gen/HostWorkerPoll").HostWorkerPoll | import("./gen/HostError").HostError;
}

// Synchronous: workspace ensure/list are cheap mux ops, not agent turns.
export interface HostWorkspace {
  ensure(opts: OptionalWireFields<import("./gen/HostWorkspaceOptions").HostWorkspaceOptions, "externalRoot">): Partial<import("./gen/HostWorkspaceResult").HostWorkspaceResult & import("./gen/HostError").HostError>;
  tabs(workspaceId: string): TabRef[];
}

// session), get (the plugin's key→session map). send/poll is the job-id bridge — the VM never blocks.
type UnionFields<T> = { [K in T extends unknown ? keyof T : never]?: T extends unknown ? K extends keyof T ? T[K] : never : never };
export type HostAgentRunReply = UnionFields<import("./gen/StepRunState").StepRunState | import("./gen/HostError").HostError>;

export interface HostAgentRun {
  start<T>(opts: (Omit<OptionalWireFields<import("./gen/StepAgentStartOptions").StepAgentStartOptions, "model" | "sandbox">, "sandbox"> & {sandbox?: Extract<"read-only", NonNullable<import("./gen/StepAgentStartOptions").StepAgentStartOptions["sandbox"]>>}), then: (reply: Partial<import("./gen/HostRunStart").HostRunStart & import("./gen/HostError").HostError>) => T): T;
  await<T>(run: string, then: (reply: HostAgentRunReply) => T): T;
  cancel<T>(run: string, then: (reply: HostAgentRunReply) => T): T;
}

export interface HostAgent {
  spawn(
    workspaceId: string,
    opts: OptionalWireFields<import("./gen/HostAgentSpawnOptions").HostAgentSpawnOptions, "provider" | "model" | "key" | "commands">,
  ): Partial<import("./gen/HostSessionResult").HostSessionResult & import("./gen/HostError").HostError>;
  resume(workspaceId: string, sessionId: string): Partial<import("./gen/HostSessionResult").HostSessionResult & import("./gen/HostError").HostError> | null;
  get(workspaceId: string, key: string): Partial<import("./gen/HostSessionResult").HostSessionResult & import("./gen/HostError").HostError> | null;
  send(sessionId: string, text: string): Partial<import("./gen/HostTicketResult").HostTicketResult & import("./gen/HostError").HostError>;
  poll(ticket: string): import("./gen/HostAgentPoll").HostAgentPoll | import("./gen/HostError").HostError;
  reap(sessionId: string): void;
  usage?(sessionId: string): UsageSnapshot | null;
}

// Synchronous: mem search is sync, so a sync stage like chatPreprocess can use the hits directly mid-turn.
export interface HostMem {
  search(opts: OptionalWireFields<import("./gen/HostMemSearchOptions").HostMemSearchOptions, "k">): import("./gen/HostMemSearchResult").HostMemSearchResult | import("./gen/HostError").HostError;
}

// ── host.analytics (machine-wide KPI aggregates; needs permissions.analytics) ──

export type AnalyticsBucket = import("./gen/AnalyticsBucket").AnalyticsBucket;
export type AnalyticsGroupBy = import("./gen/AnalyticsGroupBy").AnalyticsGroupBy;
export type AnalyticsCoverage = import("./gen/Coverage").Coverage;
export type AnalyticsDimension = import("./gen/Dimension").Dimension;
export type AnalyticsKeyReason = import("./gen/KeyReason").KeyReason;

export type AnalyticsModelKey = import("./gen/ModelKey").ModelKey;

// Local calendar days, inclusive, at most 92; day buckets at most 31.
export type AnalyticsQuery = Omit<import("./gen/AnalyticsQuery").AnalyticsQuery, "filters" | "scope"> & {filters?: Partial<import("./gen/AnalyticsQuery").AnalyticsQuery["filters"]>; scope?: Pick<NonNullable<import("./gen/AnalyticsQuery").AnalyticsQuery["scope"]>, "kind"> & Partial<Pick<NonNullable<import("./gen/AnalyticsQuery").AnalyticsQuery["scope"]>, "includes_mesh_peers">>};

export type AnalyticsTotals = import("./gen/Totals").Totals;

export type AnalyticsGroupKey = import("./gen/GroupKey").GroupKey;

export type AnalyticsGroup = import("./gen/GroupRow").GroupRow;

export type AnalyticsTokenBucket = import("./gen/TokenBucket").TokenBucket;

export type AnalyticsTabBucket = import("./gen/TabBucket").TabBucket;

export type AnalyticsAllowance = import("./gen/AllowanceEntry").AllowanceEntry;

// Every missing figure is null, never 0.
export type AnalyticsSnapshot = import("./gen/AnalyticsSnapshot").AnalyticsSnapshot;

export type AnalyticsRefusal = import("./gen/AnalyticsRefusal").AnalyticsRefusal;

// null = the host could not answer at all.
export interface HostAnalytics {
  query(query: AnalyticsQuery): AnalyticsSnapshot | AnalyticsRefusal | null;
}

// The calling plugin's own receiver binding. Only host chrome (tab menu, API, CLI) assigns a tab.
export type ReceiverTargetView = import("./gen/ReceiverTargetView").ReceiverTargetView;

export type ReceiverTargetRefusal = import("./gen/HostRefusal").HostRefusal;

export interface HostReceiverTarget {
  get(): ReceiverTargetView | ReceiverTargetRefusal | null;
  clear(): ReceiverTargetView | ReceiverTargetRefusal | null;
}

export type LocalRecordingStatus = import("./gen/RecordingStatus").RecordingStatus;

export type LocalRecordingRefusal = import("./gen/HostError").HostError;

export type RecordingDeviceSelection = import("./gen/SelectedDevices").SelectedDevices;

export type RecordingInputDevice = import("./gen/RecordingInputDevice").RecordingInputDevice;

export interface HostLocalRecording {
  listInputDevices(): RecordingInputDevice[] | LocalRecordingRefusal | null;
  selectDevices(devices: RecordingDeviceSelection): number | LocalRecordingRefusal | null;
  savedSelection(): RecordingDeviceSelection | LocalRecordingRefusal | null;
  statusReport(): number[] | LocalRecordingRefusal | null;
  status(): LocalRecordingStatus | LocalRecordingRefusal | null;
  setEnabled(enabled: boolean): LocalRecordingStatus | LocalRecordingRefusal | null;
}

// Acts on the calling agent's authorized surface; only inside onAgentCommand, with the `surfaces` grant.
export interface HostSurface {
  dispatch(verb: string, argsJson?: string | string[] | OptionalWireFields<import("./gen/HostSurfaceArguments").HostSurfaceArguments, "of">): import("./gen/HostSurfaceResult").HostSurfaceResult | import("./gen/HostError").HostError;
  // Returns the JSON text `{ "result": string }` or `{ "error": string }`.
  call(verb: string, argsJson: string): string;
  /** Identity only; plugin identity comes from the sandbox. Returns {ok:true} or {error}. */
  invalidate(identity: (import("./gen/ViewIdentity").ViewIdentity)): import("./gen/HostOk").HostOk | import("./gen/HostError").HostError;
}

export type HostProgressOpts = OptionalWireFields<import("./gen/ProgressOpts").ProgressOpts, "percent" | "done" | "error">;

export interface Host {
  io: import("./io").HostIo;
  commandTranscriptStdout(recordJson: string, declarationJson: string): string | null;
  platform(): string;
  homeDir(): string | null;
  /** Instance data root; returning the path grants no filesystem access. */
  dataDir(): string;
  expandHome(path: string): string | null;
  envGet(name: string): string | null;
  providerVersionJson(): string;
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
  writeFileAtomic(path: string, contents: string): boolean;
  canonicalPath(path: string): string | null;
  projectWriteAllowed(root: string): boolean;
  jsonBooleanRecord(path: string, keysJson: string, grant: boolean): string;
  lockFile(path: string): string | null;
  unlockFile(id: string): boolean;
  removeFile(path: string): boolean;
  makeDirs(path: string): boolean;
  isSymlink(path: string): boolean | null;
  readDirJson(path: string): string | null;
  walkDirJson(path: string, maxDepth: number): string | null;
  writeTempFile(contents: string, suffix?: string): string | null;
  sqliteQueryAllJson(db: string, sql: string, paramsJson?: string | null): string | null;
  sqliteQueryStart(db: string, sql: string, paramsJson?: string | null): string;

  shellQuote(s: string): string;
  shellQuoteFor(s: string, platform: string): string;
  shellEnvPrefix(name: string, value: string, platform: string): string;

  log(level: "trace" | "debug" | "info" | "warn" | "error", msg: string): void;

  settingsJson(): string;
  exec: HostExec;
  execStart(optsJson: string): string;
  execPoll(jobId: string): string;
  execClose(jobId: string): void;
  // The VM must yield while the job completes so a plugin cannot block other calls.
  awaitJob<T, R = ExecPollResult>(jobId: string, then: (result: R) => T): T;
  transfer: import("./binaryTransfer").HostTransfer;
  transferStart(optsJson: string): string;
  transferAbort(jobId: string): boolean;
  transferBindInput(requestJson: string): string;
  fetch: HostFetch;
  fetchStart(optsJson: string): string;
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
  agentRun: HostAgentRun;
  mem: HostMem;
  analyticsQuery(queryJson: string): string;
  analytics: HostAnalytics;
  receiverTargetGet(): string;
  receiverTargetClear(): string;
  receiverTarget: HostReceiverTarget;
  localRecordingStatus(): string;
  localRecordingSetEnabled(enabled: boolean): string;
  localRecording: HostLocalRecording;
  surfaceCall(verb: string, argsJson: string): string;
  surface: HostSurface;
}

declare global {
  // QuickJS injects `host` as a global before the plugin body runs.
  const host: Host;
}
