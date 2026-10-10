// @codeterm/plugin-sdk — plugin contracts and pure recorded-diff mapping helpers.
export type {ProviderAuthStatus, ProviderSessionObservations} from "./providerSignals";
export type {
  Host,
  HostFs,
  HostShell,
  HostPath,
  DirEntry,
  ExecConversationStep,
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
  HostAnalytics,
  AnalyticsBucket,
  AnalyticsGroupBy,
  AnalyticsCoverage,
  AnalyticsDimension,
  AnalyticsKeyReason,
  AnalyticsModelKey,
  AnalyticsQuery,
  AnalyticsTotals,
  AnalyticsGroupKey,
  AnalyticsGroup,
  AnalyticsTokenBucket,
  AnalyticsTabBucket,
  AnalyticsAllowance,
  AnalyticsSnapshot,
  AnalyticsRefusal,
  HostReceiverTarget,
  HostSurface,
  ReceiverTargetView,
  ReceiverTargetRefusal,
} from "./host";
export * from "./capabilities";
export type { ProviderCompatibilityManifest, ProviderCompatibilityReason } from "./providerCompatibility";
export type { ChatMsg } from "./gen/ChatMsg";
export { recordedTextDiff, recordedPatchDiff, type RecordedFileDiff } from "./fileDiffs";

export type { ModelProviderConfig, ModelProviderDefinition, ModelPreset } from './modelProviders';
