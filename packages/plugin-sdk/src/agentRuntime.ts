import type { ChatMsg } from "./gen/ChatMsg";
import type { ChatAttachment } from "./gen/ChatAttachment";
import type { ParsedSessionRecord, SessionRecordsMeta } from "./jsonlRecords";
import type { HostOwnedCapabilities } from "./capabilities";
import type { RuntimeProfileLimitsSource, RuntimeRegistryDefaults } from "./limitsSource";

export type JsonValue = null | boolean | number | string | JsonValue[] | { [key: string]: JsonValue };
export type RpcId = string | number;
export interface RpcError { code: number; message: string; data?: JsonValue }
export type RpcReply = {kind:"success"; value:JsonValue} | {kind:"error"; error:RpcError};
export type RuntimeFrame =
  | {kind:"notification"; method:string; params:JsonValue}
  | {kind:"request"; requestId:RpcId; method:string; params:JsonValue};

export interface RuntimeServiceTier { id:string; name:string; description:string; unavailableReason?:string }
export interface RuntimeServiceTierSelection { id:string|null }
export interface RuntimeCommand { name:string; description:string; argumentHint?:string }
export interface RuntimeConfigOption { id:string; name:string; category?:string; currentValue:string; options:readonly {value:string; name:string}[] }
export interface RuntimeAuthentication { state:"authenticated"|"needs_auth"|"unavailable"|"unknown"; methods:readonly {id:string; name:string; available:boolean}[] }
export interface RuntimeSettings { commands?:readonly RuntimeCommand[]; configOptions?:readonly RuntimeConfigOption[]; modes?:readonly {id:string; name:string}[]; serviceTier?:RuntimeServiceTierSelection; serviceTierAvailability?:Record<string,readonly RuntimeServiceTier[]>; model?:string; effort?:string; mode?:string; additionalDirectories?:readonly string[]; networkAccess?:boolean }
export interface TurnInput { text:string; attachments:readonly ChatAttachment[] }
export type PromptAnswer =
  | {kind:"choice"; values:readonly string[]}
  | {kind:"text"; text:string}
  | {kind:"cancel"};
export interface PendingPrompt {
  question:string;
  options:readonly {label:string; value:string}[];
  sections?:readonly {label:string; active?:boolean; completed?:boolean}[];
  multi_select?:boolean;
  submit_available?:boolean;
  selected_values?:readonly string[];
}
/** What an agent permission request would do; the host answers it from the tab's launch policy. */
export type RuntimePermissionAction = "read" | "write" | "command" | "other";
export type RuntimePermissionDecision = "allow_once" | "allow_always" | "reject_once" | "reject_always";
export interface RuntimePermission {
  action:RuntimePermissionAction;
  options:readonly {value:string; decision:RuntimePermissionDecision}[];
}
export interface TokenUsage {
  cost_micros?:number;
  input_tokens:number;
  output_tokens:number;
  cache_read_tokens?:number;
  cache_write_tokens?:number;
  used_tokens?:number;
  window_tokens?:number;
}
export interface OpenItem { turnId:string; item:ChatMsg }
export type RuntimeFailureCode =
  | "unsupported" | "invalid_schema" | "size_limit" | "hook_failed"
  | "method_not_allowed" | "binding_mismatch" | "deadline"
  | "authentication_required" | "process_unavailable" | "protocol_error" | "provider_refused" | "thread_not_found" | "session_load_refused" | "runtime_start_unbound";
export interface RuntimeFailure { code:RuntimeFailureCode; message:string }
export interface RuntimeClientBridges {fsRead:boolean; fsWrite:boolean; terminal:boolean}
export interface RuntimeHostMcp {name:string; command:string; args:readonly string[]; env?:Readonly<Record<string,string>>}
export interface RuntimeStreamWindow {
  frameSequences?:readonly number[];
  turnRequests?:readonly {sendId:string;turnId:string;afterSequence?:number;written?:boolean}[];
}
export interface RuntimeContext {
  restoredRowOrdinal?:number;
  streamWindow?:RuntimeStreamWindow;
  hostMcp?:RuntimeHostMcp;
  clientBridges?:RuntimeClientBridges;
  restrictedCli?:RestrictedCliPolicy;
  epoch:number;
  cwd:string;
  hostVersion:string;
  activeTurns:readonly RuntimeActiveTurn[];
  threadParents:readonly RuntimeThreadParent[];
  claimedSends:readonly {sendId:string; threadId:string; input:TurnInput}[];
  tabId:string;
  bindingNonce:string;
  hostOwned?:HostOwnedCapabilities;
  threadId?:string;
  activeTurnId?:string;
  settings:RuntimeSettings;
  providerSettings:JsonValue;
  pendingRequest?: {requestId:RpcId; method:string; params:JsonValue};
}
export interface RestrictedCliPolicy {
  tabId:string; bindingNonce:string; epoch:number; cwd:string;
  metadataDirectory:string;
  executable:string; executableSha256:string; configSha256:string; ruleSha256:string;
}
export type HistoryOrder = "asc" | "desc";
export interface RuntimeBackgroundProcess {processId:string; itemId:string; command:string; cwd:string; osPid:number|null}
export type RuntimeBackgroundProcessAction = {kind:"list"; cursor:string|null} | {kind:"terminate"; processId:string} | {kind:"clean"};
export type Operation =
  | {kind:"backgroundProcesses"; threadId:string; action:RuntimeBackgroundProcessAction}
  | {kind:"initialize"}
  | {kind:"openThread"; instructions:string; settings:RuntimeSettings}
  | {kind:"forkThread"; sourceThreadId:string; turnId:string}
  | {kind:"resumeThread"; threadId:string; instructions:string; settings:RuntimeSettings}
  | {kind:"readHistory"; threadId:string; cursor?:string; order?:HistoryOrder}
  | {kind:"submitTurn"; threadId:string; activeTurnId?:string;
      sendId:string; input:TurnInput; settings:RuntimeSettings}
  | {kind:"interrupt"; threadId:string; activeTurnId?:string}
  | {kind:"answer"; requestId:RpcId; answer:PromptAnswer}
  | {kind:"answerQuestion"; threadId:string; itemId:string; answers:readonly RuntimeQuestionAnswer[]}
  | {kind:"control"; threadId?:string; action:"restoreContext"; value:RuntimeRestoreContext}
  | {kind:"control"; threadId?:string; action:string; value:JsonValue};
export interface RpcCall { method:string; params:JsonValue; target?:{threadId:string;turnId?:string} }
export type RuntimeAction =
  | {kind:"local"}
  | {kind:"refused"; reason:string}
  | {kind:"request"; call:RpcCall}
  | {kind:"notification"; notification:RpcCall}
  | {kind:"reply"; requestId:RpcId; result:RpcReply}
  | {kind:"questionAnswer"; input:TurnInput};
export interface RuntimePlan { action:RuntimeAction; state:JsonValue }
export type RuntimeGoalAction = "read" | "cancel" | "pause" | "resume" | "complete";
export type OperationResult =
  | {kind:"goal"; threadId:string; intent:import("./gen/ChatSessionIntent").ChatSessionIntent|null}
  | {kind:"backgroundProcesses"; threadId:string; processes:readonly RuntimeBackgroundProcess[]; nextCursor:string|null}
  | {kind:"backgroundProcessTermination"; threadId:string; processId:string; acknowledged:boolean}
  /** midTurnInput: this agent accepts input into a running turn, so the host stops holding it for the turn's end. */
  | {kind:"initialized"; binaryVersion?:string; agentName?:string; midTurnInput?:boolean}
  | {kind:"threadLoaded"; threadId:string; settings:RuntimeSettings}
  | {kind:"threadForked"; sourceThreadId:string; threadId:string; turnId:string}
  | {kind:"historyPage"; threadId:string; items:readonly ChatMsg[];
      nextCursor?:string; openItems:readonly OpenItem[]; questions?:readonly RuntimeHistoryQuestion[];
      /** Echo of the requested order; absent means ascending. */
      order?:HistoryOrder}
  | {kind:"turnAccepted"; threadId:string; sendId:string; turnId?:string}
  | {kind:"resetCreditConsumed"; outcome:ResetCreditOutcome}
  | {kind:"done"};
export type RuntimeStep =
  | {kind:"continue"; plan:RuntimePlan; beforeAcceptance?:boolean; history?:Extract<OperationResult,{kind:"historyPage"}>}
  | {kind:"complete"; result:OperationResult; notification?:RpcCall; events?:readonly AgentEvent[]}
  | {kind:"refused"; reason:RuntimeFailure; beforeAcceptance:boolean};
export type AgentEvent =
  | {kind:"authentication"; authentication:RuntimeAuthentication}
  | {kind:"notice"; threadId:string; message:string}
  | {kind:"backgroundProcesses"; threadId:string; processes:readonly RuntimeBackgroundProcess[]}
  | {kind:"chatItem"; threadId:string; item:ChatMsg}
  | {kind:"chatItemPatch"; threadId:string; item:ChatMsg}
  | {kind:"chatItemAppend"; threadId:string; itemId:string; content:string; offset:number}
  | {kind:"turnState"; threadId:string; turnId?:string; state:"working"|"idle"|"needs_input"; detail?:RuntimeTurnDetail; statusLine?:string; startedAt?:number; completedAt?:number; parentThreadId?:string}
  | {kind:"sendEvidence"; threadId:string; sendId:string; turnId:string; itemId?:string}
  | {kind:"usage"; threadId:string; usage:TokenUsage}
  | {kind:"context"; threadId:string; used_tokens:number; window_tokens:number; cost_micros?:number}
  | {kind:"prompt"; threadId:string; requestId:RpcId; prompt:PendingPrompt; permission?:RuntimePermission}
  | {kind:"promptResolved"; threadId:string; requestId:RpcId}
  | {kind:"controlsConfirmed"; threadId:string; settings:RuntimeSettings}
  | {kind:"contextCompacted"; threadId:string; itemId:string}
  | {kind:"threadReverted"; threadId:string}
  | {kind:"question"; threadId:string; itemId:string; questions:readonly RuntimeQuestion[]; response:"message"}
  | {kind:"authLogin"; login:RuntimeLoginState}
  | {kind:"auth"; state:"signedIn"|"signedOut"|"unknown"; billingMode?:"subscription"|"api"|"unknown"; detail?:string}
  | {kind:"rateLimits"; threadId?:string; rateLimits:readonly RuntimeRateLimit[]; resets?:ResetCredits; accountCredits?:RuntimeAccountCredits};
export interface ResetCredit {id:string; status:"available"|"redeeming"|"redeemed"|"unknown"; expiresAt:number|null; title?:string; description?:string}
export interface ResetCredits {availableCount:number; nextExpiry:number|null; credits:readonly ResetCredit[]|null}
export type RuntimeAccountCredits = {kind:"balance"; balance:string} | {kind:"unlimited"};
export type ResetCreditOutcome = "reset"|"nothingToReset"|"noCredit"|"alreadyRedeemed";
export interface RuntimeReduction { state:JsonValue; events:readonly AgentEvent[] }
export type AgentRuntimeTransport = "stdio-jsonrpc" | "http-sse";
export interface RuntimeLaunchProfile {
  icon?:string;
  /** Host-sanitized bundle asset; never accepted from manifest JSON. */
  iconSvg?:string;
  registryId?:string;
  iconUrl?:string;
  binary:string;
  args:readonly string[];
  env?:Readonly<Record<string,string>>;
  /** The agent process starts in a host-owned directory under the profile's data; sessions keep the tab's directory. */
  hostWorkingDirectory?:boolean;
  install?:{package:string; registry?:string; distribution?:"npx"|"uvx"|"binary"; url?:string; sha256?:string; executable?:string};
  detection?:{args:readonly string[]};
  secretEnv?:Readonly<Record<string,string>>;
  /** Optional credentials: injected only when stored; the listed declared sign-in methods take an API key. */
  authEnv?:Readonly<Record<string,{secret:string; methods:readonly string[]}>>;
  ownerConfigured?:boolean;
  clientCapabilities?:{fs?:{readTextFile?:boolean; writeTextFile?:boolean}; terminal?:boolean};
  limitsSource?:RuntimeProfileLimitsSource;
  /** The agent's own session mode that approves every action, entered by full-access launches. */
  fullAccessMode?:string;
  /** How far the agent's own permission questions reach; absent makes no claim about them. */
  askScope?:RuntimeAskScope;
}
/** `outsideWorkspace`: the agent confines its own work to the workspace and asks only to go beyond it. */
export type RuntimeAskScope = "outsideWorkspace";
export interface StdioAgentRuntimeCapability {
  hostMcp?:{param:string; methods:readonly string[]};
  envelope?:"jsonrpc2";
  interrupt?:{beforeFirstTurn:boolean};
  clientBridges?:RuntimeClientBridges;
  launchProfiles?:Readonly<Record<string,RuntimeLaunchProfile>>;
  registryDefaults?:Readonly<Record<string,RuntimeRegistryDefaults>>;
  restrictedCli?:{kind:"execpolicy-service-operations";metadataDirectory:string};
  runtimeModes?:readonly {id:string; displayName:string}[];
  /** Permission requests declare their option kinds, so the host answers them from the tab's effective choice. */
  hostApprovals?:boolean;
  serviceTiers?:readonly RuntimeServiceTier[];
  backgroundProcesses?:boolean;
  goals?:{actions:readonly RuntimeGoalAction[]};
  threadHistory?:{actions:readonly ("fork"|"revert")[]};
  resetCreditsMinVersion?:string;
  storedHistory?:boolean;
  usage?:boolean;
  runtime:AgentRuntimeTransport;
  command:{binary:string; args:readonly string[]};
  login?: {methods:readonly RuntimeLoginMethod[]; terminalCommand?:string};
  inheritedEnvironmentExclusions?:readonly string[];
  turnScopedRequests?:readonly "submitTurn"[];
  requestMethods:Partial<Record<Operation["kind"], readonly string[]>>;
  notificationMethods:readonly string[];
}
export interface AgentRuntime {
  runtimeStoredHistoryAvailable?(cwd:string, sessionId:string): boolean;
  runtimeStoredHistory?(sessionId:string, records:string): readonly ChatMsg[];
  runtimeStoredRecords?(sessionId:string, records:readonly ParsedSessionRecord[], meta:SessionRecordsMeta): readonly ChatMsg[];
  runtimePlan(ctx:RuntimeContext, op:Operation): RuntimePlan;
  runtimeResponse(ctx:RuntimeContext, op:Operation, state:JsonValue, reply:RpcReply): RuntimeStep;
  runtimeReduce(ctx:RuntimeContext, state:JsonValue, frames:readonly RuntimeFrame[]): RuntimeReduction;
}
export type RuntimeLifecycleEvent =
  | {kind:"spawnFailed"; epoch:number; reason:string}
  | {kind:"processExited"; epoch:number; code?:number; signal?:number; stderrTail:string; requested:boolean}
  | {kind:"pipeEof"; epoch:number; stream:"stdout"}
  | {kind:"pipeFailure"; epoch:number; stream:"stdin"|"stdout"; reason:string}
  | {kind:"initialized"; epoch:number}
  | {kind:"threadUnloaded"; epoch:number; threadId:string; reason:string};

export interface RuntimeRateLimit {window:string; usedPercent:number; resetsAt?:number; windowMinutes?:number}
export type HostAgentEvent = AgentEvent & {at:number};
export type HostAgentRuntimeEvent = HostAgentEvent;

export interface RuntimeQuestion {id:string; text:string; options:readonly {value:string;label:string}[]; multiSelect?:boolean; allowFreeText?:boolean}
export interface RuntimeQuestionAnswer {questionId:string; values:readonly string[]; text?:string}

/** working: compacting; needs_input: awaitingApproval | awaitingInput | systemError; idle: interrupted | failed. */
export type RuntimeTurnDetail = "compacting" | "awaitingApproval" | "awaitingInput" | "systemError" | "interrupted" | "failed";

export interface RuntimeActiveTurn {threadId:string; turnId:string; parentThreadId?:string}

export interface RuntimeThreadParent {threadId:string; parentThreadId:string}

export interface RuntimeHistoryQuestion {threadId:string; itemId:string; questions:readonly RuntimeQuestion[]; response:"message"; resolved?:boolean}

export type RuntimeRestoreContext = {instructions:string|null; additionalContext:readonly RuntimeAdditionalContext[]}
export type RuntimeAdditionalContext = {key:string; kind:"application"|"user"; value:string}

export type RuntimeLoginMethod = "browser"|"device"|"apiKey";
export interface RuntimeLoginState {status:"pending"|"completed"|"failed"|"canceled"; loginId?:string; method?:RuntimeLoginMethod; url?:string; userCode?:string; expiresAtMs?:number|null; error?:string}
