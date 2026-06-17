// @codeterm/plugin-sdk — type-only SDK for authoring CodeTerm plugin logic in TS.
// Importing this adds NO runtime weight: it only declares the `host` global and
// the capability contracts. QuickJS lacks console/fetch/timers — keep it typed-only.
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
  Backend,
  PaneRef,
  UsageSnapshot,
  HostWorker,
  HostWorkspace,
  HostAgent,
  HostMem,
} from "./host";
export * from "./capabilities";
