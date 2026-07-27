// @codeterm/plugin-sdk — type-only SDK for authoring CodeTerm plugin logic in TS.
export type {
  Host,
  HostFs,
  HostShell,
  HostPath,
  DirEntry,
  ExecOpts,
  ExecResult,
  FetchOpts,
  FetchResult,
  FetchStreamStart,
  FetchStreamPoll,
  HostFetch,
  Backend,
  TabRef,
  UsageSnapshot,
  HostWorker,
  HostWorkspace,
  HostAgent,
  HostMem,
} from "./host";
export * from "./capabilities";
