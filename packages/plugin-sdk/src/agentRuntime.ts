import type { ChatMsg } from "./gen/ChatMsg";
import type { ChatAttachment } from "./gen/ChatAttachment";

export type JsonValue = null | boolean | number | string | JsonValue[] | { [key: string]: JsonValue };
export type RpcId = string | number;
export interface RpcError { code: number; message: string; data?: JsonValue }
export type RpcReply = {kind:"success"; value:JsonValue} | {kind:"error"; error:RpcError};
export type RuntimeFrame =
  | {kind:"notification"; method:string; params:JsonValue}
  | {kind:"request"; requestId:RpcId; method:string; params:JsonValue};

export interface RuntimeSettings { model?:string; effort?:string; mode?:string }
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
export interface TokenUsage {
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
  | "process_unavailable" | "protocol_error" | "provider_refused";
export interface RuntimeFailure { code:RuntimeFailureCode; message:string }
export interface RuntimeContext {
  epoch:number;
  cwd:string;
  hostVersion:string;
  activeTurns:readonly RuntimeActiveTurn[];
  threadParents:readonly RuntimeThreadParent[];
  claimedSends:readonly {sendId:string; threadId:string; input:TurnInput}[];
  tabId:string;
  bindingNonce:string;
  threadId?:string;
  activeTurnId?:string;
  settings:RuntimeSettings;
  providerSettings:JsonValue;
  pendingRequest?: {requestId:RpcId; method:string; params:JsonValue};
}
export type Operation =
  | {kind:"initialize"}
  | {kind:"openThread"; instructions:string; settings:RuntimeSettings}
  | {kind:"resumeThread"; threadId:string; settings:RuntimeSettings}
  | {kind:"readHistory"; threadId:string; cursor?:string}
  | {kind:"submitTurn"; threadId:string; activeTurnId?:string;
      sendId:string; input:TurnInput; settings:RuntimeSettings}
  | {kind:"interrupt"; threadId:string; activeTurnId?:string}
  | {kind:"answer"; requestId:RpcId; answer:PromptAnswer}
  | {kind:"answerQuestion"; threadId:string; itemId:string; answers:readonly RuntimeQuestionAnswer[]}
  | {kind:"control"; threadId?:string; action:string; value:JsonValue};
export interface RpcCall { method:string; params:JsonValue; target?:{threadId:string;turnId?:string} }
export type RuntimeAction =
  | {kind:"request"; call:RpcCall}
  | {kind:"notification"; notification:RpcCall}
  | {kind:"reply"; requestId:RpcId; result:RpcReply}
  | {kind:"questionAnswer"; input:TurnInput};
export interface RuntimePlan { action:RuntimeAction; state:JsonValue }
export type OperationResult =
  | {kind:"initialized"}
  | {kind:"threadLoaded"; threadId:string; settings:RuntimeSettings}
  | {kind:"historyPage"; threadId:string; items:readonly ChatMsg[];
      nextCursor?:string; openItems:readonly OpenItem[]}
  | {kind:"turnAccepted"; threadId:string; sendId:string; turnId?:string}
  | {kind:"done"};
export type RuntimeStep =
  | {kind:"continue"; plan:RuntimePlan; beforeAcceptance?:boolean}
  | {kind:"complete"; result:OperationResult; notification?:RpcCall; events?:readonly AgentEvent[]}
  | {kind:"refused"; reason:RuntimeFailure; beforeAcceptance:boolean};
export type AgentEvent =
  | {kind:"chatItem"; threadId:string; item:ChatMsg}
  | {kind:"turnState"; threadId:string; turnId?:string; state:"working"|"idle"|"needs_input"; startedAt?:number; completedAt?:number; parentThreadId?:string}
  | {kind:"sendEvidence"; threadId:string; sendId:string; turnId:string}
  | {kind:"usage"; threadId:string; usage:TokenUsage}
  | {kind:"prompt"; threadId:string; requestId:RpcId; prompt:PendingPrompt}
  | {kind:"promptResolved"; threadId:string; requestId:RpcId}
  | {kind:"controlsConfirmed"; threadId:string; settings:RuntimeSettings}
  | {kind:"contextCompacted"; threadId:string; itemId:string}
  | {kind:"question"; threadId:string; itemId:string; questions:readonly RuntimeQuestion[]; response:"message"}
  | {kind:"auth"; state:"signedIn"|"signedOut"|"unknown"; detail?:string}
  | {kind:"rateLimits"; threadId?:string; rateLimits:readonly RuntimeRateLimit[]};
export interface RuntimeReduction { state:JsonValue; events:readonly AgentEvent[] }
export interface StdioAgentRuntimeCapability {
  runtime:"stdio-jsonrpc";
  command:{binary:string; args:readonly string[]};
  inheritedEnvironmentExclusions?:readonly string[];
  requestMethods:Partial<Record<Operation["kind"], readonly string[]>>;
  notificationMethods:readonly string[];
}
export interface AgentRuntime {
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

export interface RuntimeActiveTurn {threadId:string; turnId:string; parentThreadId?:string}

export interface RuntimeThreadParent {threadId:string; parentThreadId:string}
