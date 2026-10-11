import { Bell, Bot, CalendarClock, CircleStop, Clock3, Cpu, Ellipsis, FileInput, GitBranch, Hand, MessageSquareReply, Play, Radio, Send, ShieldAlert, SquareTerminal, Workflow, Zap, type LucideIcon } from "lucide-react";
import type { BlueprintNode } from "./blueprintModel";

export const blueprintNodeKinds = {
  trigger: { label: "Trigger", width: 196, icon: Zap },
  source: { label: "Source", width: 248, icon: Cpu },
  agent: { label: "Agent", width: 288, icon: Bot },
  logic: { label: "Logic", width: 264, icon: GitBranch },
  action: { label: "Action", width: 232, icon: Zap },
} as const;

export type BlueprintNodeType = keyof typeof blueprintNodeKinds;
export const blueprintLegend = Object.keys(blueprintNodeKinds) as BlueprintNodeType[];

export const blueprintTriggers = {
  schedule: { catalogType: "schedule", icon: Clock3, purpose: "Starts this stage on its schedule." },
  event: { catalogType: "event", icon: Radio, purpose: "Starts when the watched source reports new activity." },
  manual: { catalogType: "input", icon: Hand, purpose: "Starts when text arrives through codeterm blueprint input." },
  activated: { catalogType: "activated", icon: Workflow, purpose: "Starts when an earlier stage activates it." },
} as const;
export type TriggerKind = keyof typeof blueprintTriggers;

export const blueprintSources = {
  probe: { catalogType: "probe", icon: SquareTerminal },
  terminal: { catalogType: "terminal", icon: SquareTerminal },
  observe: { catalogType: "observe", icon: Radio },
  input: { catalogType: "text_input", icon: FileInput },
} as const;
export type SourceKind = keyof typeof blueprintSources;

interface ActionKind {
  icon: LucideIcon;
  title: (target: string | null, label: string | null) => string;
  chain?: boolean;
  code?: boolean;
}

export const blueprintActions = {
  notify: { icon: Bell, title: () => "Notify owner" },
  send: { icon: Send, title: (target) => `Send to ${target ?? "agent"}` },
  cron: { icon: CalendarClock, title: (target) => `Start ${target ?? "cron job"}` },
  command: { icon: SquareTerminal, title: (_, label) => `Run ${label ?? "command"}`, code: true },
  blueprint: { icon: Play, title: (target) => `Run ${target ?? "next Blueprint"}`, chain: true },
  escalate: { icon: ShieldAlert, title: (target) => `Escalate to ${target ?? "creator"}` },
  respond_prompt: { icon: MessageSquareReply, title: () => "Answer prompt" },
  stop: { icon: CircleStop, title: () => "Stop Blueprint" },
} satisfies Record<string, ActionKind>;

export function actionKind(kind: string): ActionKind | undefined {
  return Object.prototype.hasOwnProperty.call(blueprintActions, kind) ? blueprintActions[kind as keyof typeof blueprintActions] : undefined;
}

export function actionTitle(kind: string, target: string | null, label: string | null): string {
  return actionKind(kind)?.title(target, label) ?? kind;
}

export const blueprintSteps = {
  agent: { type: "agent", icon: Bot, label: "Agent" },
  command: { type: "source", icon: SquareTerminal, label: "Command" },
  convert: { type: "source", icon: Workflow, label: "Convert" },
  ask: { type: "agent", icon: Bot, label: "Ask" },
  filter: { type: "logic", icon: GitBranch, label: "Filter" },
} as const;
export type BlueprintStepKind = keyof typeof blueprintSteps;

export function nodeIcon(node: BlueprintNode): LucideIcon {
  if (node.type === "trigger") return blueprintTriggers[node.trigger ?? "schedule"].icon;
  if (node.type === "source") return blueprintSources[node.source ?? "probe"].icon;
  if (node.type === "action") return node.action?.overflow ? Ellipsis : actionKind(node.action?.handler?.type ?? "")?.icon ?? Zap;
  return blueprintNodeKinds[node.type].icon;
}
