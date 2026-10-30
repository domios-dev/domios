import { GitHubMark } from "../GitHubMark";
import { Bell, Bot, Braces, CalendarClock, CircleStop, Clock3, Cpu, FileInput, Flag, GitBranch, Globe, Hand, MessageSquareReply, Network, Play, Puzzle, Radio, Send, ShieldAlert, SquareTerminal, Workflow, Zap } from "lucide-react";
import type { ComponentType } from "react";
import type { BlueprintIrNode, BlueprintPluginNode } from "../../../lib/blueprintApi";

export type NodeIcon = ComponentType<{ size?: number; className?: string }>;

export const blueprintNodeKinds = {
  trigger: { label: "Trigger", width: 216, icon: Zap },
  source: { label: "Source", width: 256, icon: Cpu },
  code: { label: "Code", width: 272, icon: Braces },
  agent: { label: "Agent", width: 272, icon: Bot },
  logic: { label: "Logic", width: 248, icon: GitBranch },
  call: { label: "Sub-graph", width: 272, icon: Network },
  plugin: { label: "Plugin", width: 272, icon: Puzzle },
  action: { label: "Action", width: 240, icon: Bell },
  result: { label: "Result", width: 216, icon: Flag },
} as const;

export type BlueprintNodeType = keyof typeof blueprintNodeKinds;

export const blueprintTriggers = {
  schedule: { catalogType: "schedule", icon: Clock3 },
  event: { catalogType: "event", icon: Radio },
  manual: { catalogType: "input", icon: Hand },
  activated: { catalogType: "activated", icon: Workflow },
  webhook: { catalogType: "webhook", icon: Globe },
} as const;

export const blueprintSources = {
  probe: { catalogType: "probe", icon: SquareTerminal },
  terminal: { catalogType: "terminal", icon: SquareTerminal },
  observe: { catalogType: "observe", icon: Radio },
  input: { catalogType: "text_input", icon: FileInput },
} as const;

export const blueprintActions = {
  notify: { icon: Bell, label: "Notify owner" },
  send: { icon: Send, label: "Send to agent" },
  cron: { icon: CalendarClock, label: "Start cron job" },
  command: { icon: SquareTerminal, label: "Run command" },
  blueprint: { icon: Play, label: "Run Blueprint" },
  escalate: { icon: ShieldAlert, label: "Escalate" },
  respond_prompt: { icon: MessageSquareReply, label: "Answer prompt" },
  stop: { icon: CircleStop, label: "Stop Blueprint" },
} as const;

export const blueprintSteps = {
  http: { type: "source", icon: Globe, label: "HTTP health" },
  github: { type: "source", icon: GitHubMark, label: "GitHub" },
  agent: { type: "agent", icon: Bot, label: "Agent" },
  command: { type: "code", icon: SquareTerminal, label: "Command" },
  convert: { type: "code", icon: Workflow, label: "Convert" },
  ask: { type: "agent", icon: Bot, label: "Ask" },
  filter: { type: "logic", icon: GitBranch, label: "Filter" },
} as const;
export type BlueprintStepKind = keyof typeof blueprintSteps;

function own<T extends object>(table: T, key: string | undefined): T[keyof T] | undefined {
  return key !== undefined && Object.prototype.hasOwnProperty.call(table, key) ? table[key as keyof T] : undefined;
}

export interface IrNodeKind { type: BlueprintNodeType; icon: NodeIcon; label: string }

/** Classifies one Blueprint definition node by what it does, never by its id. */
export function irNodeKind(node: BlueprintIrNode, deterministicDecision: boolean): IrNodeKind {
  if (node.call) return { type: "call", icon: Network, label: "Sub-graph" };
  if (node.code) return { type: "code", icon: Braces, label: node.code.lang === "ts" ? "TypeScript" : node.code.lang.charAt(0).toUpperCase() + node.code.lang.slice(1) };
  if (node.run !== undefined) return { type: "code", icon: SquareTerminal, label: "Command" };
  const step = own(blueprintSteps, node.uses);
  if (step) return node.uses === "ask" && deterministicDecision ? { type: "logic", icon: GitBranch, label: "Classify" } : step;
  const source = own(blueprintSources, node.uses);
  if (source) return { type: "source", icon: source.icon, label: node.uses === "probe" ? "Probe" : node.uses ?? "Source" };
  const action = own(blueprintActions, node.uses);
  if (action) return { type: "action", icon: action.icon, label: action.label };
  return { type: "action", icon: Zap, label: node.uses ?? "Step" };
}

export function triggerIcon(type: unknown): NodeIcon {
  if (type === "schedule") return blueprintTriggers.schedule.icon;
  if (type === "event") return blueprintTriggers.event.icon;
  if (type === "input") return blueprintTriggers.manual.icon;
  if (type === "webhook") return blueprintTriggers.webhook.icon;
  return Zap;
}

/** A node the payload marks as an installed plugin step; the plugin's name is its kind label. */
export function pluginNodeKind(descriptor: BlueprintPluginNode): IrNodeKind {
  return { type: "plugin", icon: Puzzle, label: descriptor.available && descriptor.plugin ? descriptor.plugin : "Plugin" };
}

/** The payload's descriptor for a node's `uses`, if the payload marks it a plugin step. */
export function pluginDescriptor(node: BlueprintIrNode, nodes: Readonly<Record<string, BlueprintPluginNode>> | undefined): BlueprintPluginNode | undefined {
  return nodes ? own(nodes, node.uses) : undefined;
}
