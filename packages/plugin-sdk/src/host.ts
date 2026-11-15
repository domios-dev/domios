import type {OptionalWireFields} from "./wireViews";

export type ExecConversationStep = import("./gen/ExecConversationStep").ExecConversationStep;

export type ExecOpts = OptionalWireFields<import("./gen/ExecOpts").ExecOpts, "args" | "env" | "stdin" | "conversation" | "timeoutMs" | "detach" | "logFile">;

export type ExecResult = import("./gen/ExecResult").ExecResult;

export type ExecStartResult = Partial<import("./gen/HostJobStart").HostJobStart & import("./gen/HostError").HostError>;

export type ExecPollResult = import("./gen/ExecPollResult").ExecPollResult;

export type FetchOpts = Pick<import("./gen/FetchOpts").FetchOpts, "url"> & Partial<Omit<import("./gen/FetchOpts").FetchOpts, "url">> & {credential?: CredentialRequest; profileSecret?: ProfileSecretRequest};

export interface ProfileSecretRequest {
  profile: string;
  env: string;
  headers: Record<string, string>;
}

export interface CredentialRequest {
  id: string;
  headers: Record<string, string>;
}

export type FetchResult = import("./gen/FetchResult").FetchResult;

export type FetchStreamStart = Partial<import("./gen/HostJobStart").HostJobStart & import("./gen/HostError").HostError>;

export type FetchStreamPoll = import("./gen/StreamPollResponse").StreamPollResponse;

export type DirEntry = import("./gen/DirEntry").DirEntry;

export interface HostShell {
  quote(s: string): string;
  quoteFor(s: string, platform: string): string;
  envPrefix(name: string, value: string, platform: string): string;
}

export interface HostPath {
  isWindows: boolean;
  toForwardSlash(p: string): string;
  toNative(p: string): string;
  normalize(p: string): string;
  equal(a: string, b: string): boolean;
}

export type Backend = Partial<Pick<import("./gen/HostWorkerStartOptions").HostWorkerStartOptions, "provider" | "model">>;

export interface TabRef {
  tabId: string;
  title?: string;
  backing?: string;
}

export type UsageSnapshot = import("./gen/ProviderUsageSnapshot").ProviderUsageSnapshot;

type UnionFields<T> = { [K in T extends unknown ? keyof T : never]?: T extends unknown ? K extends keyof T ? T[K] : never : never };
export type HostAgentRunReply = UnionFields<import("./gen/StepRunState").StepRunState | import("./gen/HostError").HostError>;

export interface HostAgentRun {
  start<T>(opts: (Omit<OptionalWireFields<import("./gen/StepAgentStartOptions").StepAgentStartOptions, "model" | "sandbox">, "sandbox"> & {sandbox?: Extract<"read-only", NonNullable<import("./gen/StepAgentStartOptions").StepAgentStartOptions["sandbox"]>>}), then: (reply: Partial<import("./gen/HostRunStart").HostRunStart & import("./gen/HostError").HostError>) => T): T;
  await<T>(run: string, then: (reply: HostAgentRunReply) => T): T;
  cancel<T>(run: string, then: (reply: HostAgentRunReply) => T): T;
}

export type AnalyticsBucket = import("./gen/AnalyticsBucket").AnalyticsBucket;
export type AnalyticsGroupBy = import("./gen/AnalyticsGroupBy").AnalyticsGroupBy;
export type AnalyticsCoverage = import("./gen/Coverage").Coverage;
export type AnalyticsDimension = import("./gen/Dimension").Dimension;
export type AnalyticsKeyReason = import("./gen/KeyReason").KeyReason;

export type AnalyticsModelKey = import("./gen/ModelKey").ModelKey;

export type AnalyticsQuery = Omit<import("./gen/AnalyticsQuery").AnalyticsQuery, "filters" | "scope"> & {filters?: Partial<import("./gen/AnalyticsQuery").AnalyticsQuery["filters"]>; scope?: Pick<NonNullable<import("./gen/AnalyticsQuery").AnalyticsQuery["scope"]>, "kind"> & Partial<Pick<NonNullable<import("./gen/AnalyticsQuery").AnalyticsQuery["scope"]>, "includes_mesh_peers">>};

export type AnalyticsTotals = import("./gen/Totals").Totals;

export type AnalyticsGroupKey = import("./gen/GroupKey").GroupKey;

export type AnalyticsGroup = import("./gen/GroupRow").GroupRow;

export type AnalyticsTokenBucket = import("./gen/TokenBucket").TokenBucket;

export type AnalyticsTabBucket = import("./gen/TabBucket").TabBucket;

export type AnalyticsAllowance = import("./gen/AllowanceEntry").AllowanceEntry;

export type AnalyticsSnapshot = import("./gen/AnalyticsSnapshot").AnalyticsSnapshot;

export type AnalyticsRefusal = import("./gen/AnalyticsRefusal").AnalyticsRefusal;

export type ReceiverTargetView = import("./gen/ReceiverTargetView").ReceiverTargetView;

export type ReceiverTargetRefusal = import("./gen/HostRefusal").HostRefusal;

export type LocalRecordingStatus = import("./gen/RecordingStatus").RecordingStatus;

export type LocalRecordingRefusal = import("./gen/HostError").HostError;

export type RecordingDeviceSelection = import("./gen/SelectedDevices").SelectedDevices;

export type RecordingInputDevice = import("./gen/RecordingInputDevice").RecordingInputDevice;

export type HostProgressOpts = OptionalWireFields<import("./gen/ProgressOpts").ProgressOpts, "percent" | "done" | "error">;

export interface Host {
  io: import("./io").HostIo;
  fs: { expandHome(path: string): string | null };
  exec: { async<T>(opts: ExecOpts, then: (result: ExecPollResult) => T): T };
  fetch: { async<T>(opts: FetchOpts, then: (result: FetchResult) => T): T };
  agentRun: HostAgentRun;
  commandTranscriptStdout(recordJson: string, declarationJson: string): string | null;
  platform(): string;
  homeDir(): string | null;
  dataDir(): string;
  expandHome(path: string): string | null;
  envGet(name: string): string | null;
  unixNow(): number;
  unixNowMs(): number;
  md5(s: string): string;
  shellQuote(s: string): string;
  shellQuoteFor(s: string, platform: string): string;
  shellEnvPrefix(name: string, value: string, platform: string): string;
  log(level: "trace" | "debug" | "info" | "warn" | "error", msg: string): void;
  settingsJson(): string;
  awaitJob<T, R = ExecPollResult>(jobId: string, then: (result: R) => T): T;
  toolcallParse(rawText: string, schemaJson: string): string;
  manifestJson(): string | null;
  manifest(): unknown;
  shell: HostShell;
  path: HostPath;
  toolcall: { parse(rawText: string, schemaJson: string): string };
}

declare global {
  const host: Host;
}
