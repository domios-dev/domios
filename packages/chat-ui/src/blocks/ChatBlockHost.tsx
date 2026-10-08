import React, { createContext, useContext, useMemo } from "react";
import type { LiveBindingSource } from "./liveBinding";
import type { PluginBlockState, PluginLifecycleKind } from "./pluginBlockModel";
import type { AgentBlockState } from "./agentBlockModel";
import type { ChecklistBlockState } from "./checklistBlockModel";
import type { ChoiceBlockState } from "./choiceBlockModel";
import type { JobBlockState } from "./jobBlockModel";
import type { TasksBlockState } from "./tasksBlockModel";

/** Host services the plugin block reads and acts through; every mutation runs only from a user click. */
export interface PluginBlockHost {
  binding: LiveBindingSource<PluginBlockState>;
  run(pluginId: string, kind: PluginLifecycleKind): Promise<void>;
  openView(pluginId: string): void;
  openInPlugins(pluginId: string): void;
  renderIcon?(pluginId: string, size: number): React.ReactNode;
}

/** Answers a choice for the chat that holds it; keyed by the message the block sits in. */
export interface ChoiceBlockHost {
  binding: LiveBindingSource<ChoiceBlockState | null>;
  /** Sends the label as the user's reply; resolves false when this message was already answered. */
  choose(messageId: string, label: string): Promise<boolean>;
}

/** Live agent tabs keyed by tab id or unique prefix. */
export interface AgentBlockHost {
  binding: LiveBindingSource<AgentBlockState>;
  open(tabRef: string): void;
  stop(tabRef: string): Promise<void>;
}

/** Crons and Blueprints keyed by id or exact name; actions use the Jobs view's own commands. */
export interface JobBlockHost {
  binding: LiveBindingSource<JobBlockState>;
  open(jobRef: string): void;
  runNow(jobRef: string): Promise<void>;
  setPaused(jobRef: string, paused: boolean): Promise<void>;
  stop(jobRef: string): Promise<void>;
}

/** Checklists whose items tick from real state; `open` follows an item's deep link. */
export interface ChecklistBlockHost {
  binding: LiveBindingSource<ChecklistBlockState>;
  open(checklistId: string, itemId: string): void;
}

/** Plan tasks keyed by `tasksBlockKey`; rows come live from the plan. */
export interface TasksBlockHost {
  binding: LiveBindingSource<TasksBlockState>;
  openPlan(planId: string): void;
  openTask(planId: string, taskId: string): void;
}

/** Scope-wide services for chat blocks. Absent fields hide the matching control instead of faking it. */
export interface ChatBlockHost {
  /** Adds the block to the tab composer as context; `text` is what the agent reads. */
  askAbout?(request: { title: string; text: string }): void;
  pin?(source: string): void;
  pinned?: LiveBindingSource<boolean>;
  plugins?: PluginBlockHost;
  choice?: ChoiceBlockHost;
  agents?: AgentBlockHost;
  jobs?: JobBlockHost;
  checklists?: ChecklistBlockHost;
  tasks?: TasksBlockHost;
}

const ChatBlockHostContext = createContext<ChatBlockHost>({});

/** Nested providers merge: a tab scope adds Ask and Pin on top of the app-wide data services. */
export function ChatBlockHostProvider({ value, children }: { value: ChatBlockHost; children: React.ReactNode }) {
  const parent = useContext(ChatBlockHostContext);
  const merged = useMemo(
    () => ({ ...parent, ...Object.fromEntries(Object.entries(value).filter(([, service]) => service !== undefined)) }),
    [parent, value],
  );
  return <ChatBlockHostContext.Provider value={merged}>{children}</ChatBlockHostContext.Provider>;
}

export function useChatBlockHost(): ChatBlockHost {
  return useContext(ChatBlockHostContext);
}

const ChatBlockMessageContext = createContext<string | null>(null);

/** Names the chat message a block renders in, so one-shot blocks like a choice bind to that message. */
export function ChatBlockMessageProvider({ messageId, children }: { messageId: string; children: React.ReactNode }) {
  return <ChatBlockMessageContext.Provider value={messageId}>{children}</ChatBlockMessageContext.Provider>;
}

export function useChatBlockMessageId(): string | null {
  return useContext(ChatBlockMessageContext);
}
