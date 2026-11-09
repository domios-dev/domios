import type {OptionalWireFields} from "./wireViews";
import type { ChatMsg } from "./gen/ChatMsg";
import type { ParsedSessionRecord,SessionRecordsMeta } from "./jsonlRecords";

export type JsonValue = null | boolean | number | string | JsonValue[] | { [key: string]: JsonValue };
export type RpcId = import("./gen/RpcId").RpcId;
export type RpcError = OptionalWireFields<import("./gen/RpcFailure").RpcFailure, "data">;
export type RpcReply = import("./gen/RpcReply").RpcReply;
export type RuntimeFrame = import("./gen/RuntimeFrame").RuntimeFrame;

export type RuntimeServiceTier = OptionalWireFields<import("./gen/RuntimeServiceTier").RuntimeServiceTier, "unavailableReason">;
export type RuntimeServiceTierSelection = import("./gen/RuntimeServiceTierSelection").RuntimeServiceTierSelection;
export type RuntimeCommand = OptionalWireFields<import("./gen/RuntimeCommand").RuntimeCommand, "argumentHint">;
export type RuntimeConfigOption = Omit<import("./gen/RuntimeConfigOption").RuntimeConfigOption, "options"> & {options: Readonly<import("./gen/RuntimeConfigOption").RuntimeConfigOption["options"]>};
export type RuntimeAuthentication = Omit<import("./gen/RuntimeAuthentication").RuntimeAuthentication, "methods"> & {methods: Readonly<import("./gen/RuntimeAuthentication").RuntimeAuthentication["methods"]>};
export type RuntimeSettings = Omit<{[K in keyof import("./gen/RuntimeSettings").RuntimeSettings]?: Readonly<import("./gen/RuntimeSettings").RuntimeSettings[K]>}, "configOptions"> & {configOptions?: readonly RuntimeConfigOption[]};
export type TurnInput = Omit<import("./gen/TurnInput").TurnInput, "attachments"> & {attachments: Readonly<import("./gen/TurnInput").TurnInput["attachments"]>};
export type PromptAnswer = import("./gen/PromptAnswer").PromptAnswer;
export type PendingPrompt = Omit<OptionalWireFields<import("./gen/PendingPrompt").PendingPrompt, "sections" | "multi_select" | "submit_available" | "selected_values">, "options" | "sections" | "selected_values"> & {options: Readonly<import("./gen/PendingPrompt").PendingPrompt["options"]>; sections?: Readonly<import("./gen/PendingPrompt").PendingPrompt["sections"]>; selected_values?: Readonly<import("./gen/PendingPrompt").PendingPrompt["selected_values"]>};
/** What an agent permission request would do; the host answers it from the tab's launch policy. */
export type RuntimePermissionAction = import("./gen/RuntimePermissionAction").RuntimePermissionAction;
export type RuntimePermissionDecision = import("./gen/RuntimePermissionDecision").RuntimePermissionDecision;
export type RuntimePermission = Omit<import("./gen/RuntimePermission").RuntimePermission, "options"> & {options: Readonly<import("./gen/RuntimePermission").RuntimePermission["options"]>};
export type TokenUsage = OptionalWireFields<import("./gen/TokenUsage").TokenUsage, "cost_micros" | "cache_read_tokens" | "cache_write_tokens" | "used_tokens" | "window_tokens">;
export type OpenItem = import("./gen/OpenItem").OpenItem;
export type RuntimeFailureCode = import("./gen/RuntimeFailureCode").RuntimeFailureCode;
export type RuntimeFailure = import("./gen/RuntimeFailure").RuntimeFailure;
export type RuntimeClientBridges = import("./gen/RuntimeClientBridges").RuntimeClientBridges;
export type RuntimeHostMcp = Omit<OptionalWireFields<import("./gen/RuntimeHostMcp").RuntimeHostMcp, "env">, "args"> & {args: Readonly<import("./gen/RuntimeHostMcp").RuntimeHostMcp["args"]>};
export type RuntimeStreamWindow = Omit<OptionalWireFields<import("./gen/RuntimeStreamWindow").RuntimeStreamWindow, "frameSequences" | "turnRequests">, "frameSequences" | "turnRequests"> & {frameSequences?: Readonly<import("./gen/RuntimeStreamWindow").RuntimeStreamWindow["frameSequences"]>; turnRequests?: Readonly<import("./gen/RuntimeStreamWindow").RuntimeStreamWindow["turnRequests"]>};
export type RuntimeContext = Omit<OptionalWireFields<import("./gen/RuntimeContext").RuntimeContext, "restoredRowOrdinal" | "streamWindow" | "hostMcp" | "clientBridges" | "restrictedCli" | "hostOwned" | "threadId" | "activeTurnId" | "pendingRequest">, "settings"> & {settings: RuntimeSettings};
export type RestrictedCliPolicy = import("./gen/RestrictedCliPolicy").RestrictedCliPolicy;
export type HistoryOrder = import("./gen/HistoryOrder").HistoryOrder;
export type RuntimeBackgroundProcess = import("./gen/RuntimeBackgroundProcess").RuntimeBackgroundProcess;
export type RuntimeBackgroundProcessAction = import("./gen/RuntimeBackgroundProcessAction").RuntimeBackgroundProcessAction;
export type Operation = RuntimeAuthoringView<Exclude<import("./gen/RuntimeOperation").RuntimeOperation, {kind: "control"}>> | (Omit<Extract<import("./gen/RuntimeOperation").RuntimeOperation, {kind: "control"}>, "value" | "action"> & {action: "restoreContext"; value: RuntimeRestoreContext}) | Extract<import("./gen/RuntimeOperation").RuntimeOperation, {kind: "control"}>;
export type RpcCall = OptionalWireFields<import("./gen/RpcCall").RpcCall, "target">;
export type RuntimeAction = RuntimeAuthoringView<import("./gen/RuntimeAction").RuntimeAction>;
export type RuntimePlan = Omit<import("./gen/RuntimePlan").RuntimePlan, "action"> & {action: RuntimeAction};
export type RuntimeGoalAction = import("./gen/RuntimeGoalAction").RuntimeGoalAction;
export type OperationResult = RuntimeAuthoringView<import("./gen/RuntimeResult").RuntimeResult>;
export type RuntimeStep = (Omit<Extract<import("./gen/RuntimeStep").RuntimeStep, {kind: "continue"}>, "beforeAcceptance" | "history" | "plan"> & {plan: RuntimePlan; beforeAcceptance?: boolean; history?: Extract<OperationResult, {kind: "historyPage"}>}) | (Omit<Extract<import("./gen/RuntimeStep").RuntimeStep, {kind: "complete"}>, "result" | "events"> & {result: OperationResult; events?: readonly AgentEvent[]}) | Extract<import("./gen/RuntimeStep").RuntimeStep, {kind: "refused"}>;
export type AgentEvent = RuntimeAuthoringView<import("./gen/AgentRuntimeEvent").AgentRuntimeEvent>;
export type ResetCredit = OptionalWireFields<import("./gen/ResetCredit").ResetCredit, "title" | "description">;
export type ResetCredits = Omit<import("./gen/ResetCredits").ResetCredits, "credits"> & {credits: Readonly<import("./gen/ResetCredits").ResetCredits["credits"]>};
export type RuntimeAccountCredits = import("./gen/RuntimeAccountCredits").RuntimeAccountCredits;
export type ResetCreditOutcome = import("./gen/ResetCreditOutcome").ResetCreditOutcome;
export type RuntimeReduction = Omit<import("./gen/RuntimeReduction").RuntimeReduction, "events"> & {events: readonly AgentEvent[]};
export type AgentRuntimeTransport = import("./gen/AgentRuntimeProtocol").AgentRuntimeProtocol;
export type RuntimeLaunchProfile = Omit<OptionalWireFields<import("./gen/RuntimeLaunchProfile").RuntimeLaunchProfile, "icon" | "iconSvg" | "registryId" | "iconUrl" | "env" | "hostWorkingDirectory" | "install" | "detection" | "secretEnv" | "authEnv" | "ownerConfigured" | "clientCapabilities" | "limitsSource" | "fullAccessMode" | "askScope">, "args"> & {args: Readonly<import("./gen/RuntimeLaunchProfile").RuntimeLaunchProfile["args"]>};
/** `outsideWorkspace`: the agent confines its own work to the workspace and asks only to go beyond it. */
export type RuntimeAskScope = import("./gen/RuntimeAskScope").RuntimeAskScope;
export type StdioAgentRuntimeCapability = Omit<OptionalWireFields<import("./gen/AgentRuntimeCapability").AgentRuntimeCapability, "hostMcp" | "envelope" | "interrupt" | "clientBridges" | "launchProfiles" | "registryDefaults" | "restrictedCli" | "runtimeModes" | "hostApprovals" | "serviceTiers" | "backgroundProcesses" | "goals" | "threadHistory" | "resetCreditsMinVersion" | "storedHistory" | "usage" | "login" | "inheritedEnvironmentExclusions" | "turnScopedRequests">, "runtimeModes" | "serviceTiers" | "inheritedEnvironmentExclusions" | "turnScopedRequests" | "notificationMethods"> & {runtimeModes?: Readonly<import("./gen/AgentRuntimeCapability").AgentRuntimeCapability["runtimeModes"]>; serviceTiers?: Readonly<import("./gen/AgentRuntimeCapability").AgentRuntimeCapability["serviceTiers"]>; inheritedEnvironmentExclusions?: Readonly<import("./gen/AgentRuntimeCapability").AgentRuntimeCapability["inheritedEnvironmentExclusions"]>; turnScopedRequests?: Readonly<import("./gen/AgentRuntimeCapability").AgentRuntimeCapability["turnScopedRequests"]>; notificationMethods: Readonly<import("./gen/AgentRuntimeCapability").AgentRuntimeCapability["notificationMethods"]>};
export interface AgentRuntime {
  runtimeStoredHistoryAvailable?(cwd:string, sessionId:string): boolean;
  runtimeStoredHistory?(sessionId:string, records:string): readonly ChatMsg[];
  runtimeStoredRecords?(sessionId:string, records:readonly ParsedSessionRecord[], meta:SessionRecordsMeta): readonly ChatMsg[];
  runtimePlan(ctx:RuntimeContext, op:Operation): RuntimePlan;
  runtimeResponse(ctx:RuntimeContext, op:Operation, state:JsonValue, reply:RpcReply): RuntimeStep;
  runtimeReduce(ctx:RuntimeContext, state:JsonValue, frames:readonly RuntimeFrame[]): RuntimeReduction;
}
export type RuntimeRateLimit = OptionalWireFields<import("./gen/RuntimeRateLimit").RuntimeRateLimit, "resetsAt" | "windowMinutes">;
export type HostAgentEvent = import("./gen/HostAgentRuntimeEvent").HostAgentRuntimeEvent;
export type HostAgentRuntimeEvent = import("./gen/HostAgentRuntimeEvent").HostAgentRuntimeEvent;

export type RuntimeQuestion = Omit<OptionalWireFields<import("./gen/RuntimeQuestion").RuntimeQuestion, "multiSelect" | "allowFreeText">, "options"> & {options: Readonly<import("./gen/RuntimeQuestion").RuntimeQuestion["options"]>};
export type RuntimeQuestionAnswer = Omit<OptionalWireFields<import("./gen/RuntimeQuestionAnswer").RuntimeQuestionAnswer, "text">, "values"> & {values: Readonly<import("./gen/RuntimeQuestionAnswer").RuntimeQuestionAnswer["values"]>};

/** working: compacting; needs_input: awaitingApproval | awaitingInput | systemError; idle: interrupted | failed. */
export type RuntimeTurnDetail = import("./gen/RuntimeTurnDetail").RuntimeTurnDetail;

export type RuntimeActiveTurn = OptionalWireFields<import("./gen/RuntimeActiveTurn").RuntimeActiveTurn, "parentThreadId">;

export type RuntimeThreadParent = import("./gen/RuntimeThreadParent").RuntimeThreadParent;

export type RuntimeHistoryQuestion = Omit<Omit<OptionalWireFields<import("./gen/RuntimeHistoryQuestion").RuntimeHistoryQuestion, "resolved">, "questions"> & {questions: Readonly<import("./gen/RuntimeHistoryQuestion").RuntimeHistoryQuestion["questions"]>}, "questions"> & {questions: readonly RuntimeQuestion[]};

export type RuntimeRestoreContext = Omit<import("./gen/RuntimeRestoreContext").RuntimeRestoreContext<RuntimeAdditionalContext>, "additionalContext"> & {additionalContext: Readonly<import("./gen/RuntimeRestoreContext").RuntimeRestoreContext<RuntimeAdditionalContext>["additionalContext"]>};
export type RuntimeAdditionalContext = import("./gen/RuntimeAdditionalContext").RuntimeAdditionalContext;

export type RuntimeLoginMethod = import("./gen/RuntimeLoginMethod").RuntimeLoginMethod;
export type RuntimeLoginState = OptionalWireFields<import("./gen/RuntimeLoginState").RuntimeLoginState, "loginId" | "method" | "url" | "userCode" | "expiresAtMs" | "error">;

type RuntimeAuthoringView<T> = T extends unknown ? {[K in keyof T]: K extends "settings" ? RuntimeSettings : K extends "authentication" ? RuntimeAuthentication : K extends "prompt" ? PendingPrompt : K extends "permission" ? RuntimePermission : K extends "input" ? TurnInput : K extends "resets" ? ResetCredits : K extends "questions" ? T extends {kind: "historyPage"} ? readonly RuntimeHistoryQuestion[] : readonly RuntimeQuestion[] : T[K] extends Array<infer Item> ? readonly RuntimeAuthoringView<Item>[] : T[K]} : never;
