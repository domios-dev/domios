// @codeterm/plugin-sdk — plugin contracts and pure recorded-diff mapping helpers.
export type {ProviderAuthStatus, ProviderSessionObservations, RuntimeErrorSignal} from "./providerSignals";
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
  HostLocalRecording,
  RecordingDeviceSelection,
  RecordingInputDevice,
  LocalRecordingStatus,
  LocalRecordingRefusal,
  HostSurface,
  ReceiverTargetView,
  ReceiverTargetRefusal,
} from "./host";
export * from "./capabilities";
export * from "./blueprintSteps";
export type { ProviderCompatibilityManifest, ProviderCompatibilityReason } from "./providerCompatibility";
export type { ChatMsg } from "./gen/ChatMsg";
export { recordedTextDiff, recordedPatchDiff, type RecordedFileDiff } from "./fileDiffs";

export type { ModelProviderConfig, ModelProviderDefinition, ModelPreset } from './modelProviders';

export * from "./agentRuntime";
export * from "./remoteTasks";
export {RemoteTaskFileStorage} from "./remoteTaskStorage";
export type { ParsedSessionRecord, JsonlRecordFilter, JsonlRecordSource, JsonlRecordSources, SessionRecordsMeta } from "./jsonlRecords";
export type { ExecutionSandboxCapability, ExecutionSandboxMode } from "./executionSandbox";
export { activityLabel, activityCommand, type ActivityTool } from "./activityLabel";
export type { ChatAttachment } from "./gen/ChatAttachment";

export * from "./mediaDelivery";
