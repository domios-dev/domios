import { toolCommand, type ToolEvent } from "@codeterm/chat-core";
import type { CodetermInvocation } from "../codeterm/recognize";
import { commandShape } from "../codeterm/commandShape";
import { isHelp } from "../codeterm/templates";
import { toolKindVisual, toolTones } from "./toolKindVisual";
import {
  Activity, Bot, BookOpen, Brain, CalendarClock, CheckCircle2, LogOut, Bell, Check, CircleX, LoaderCircle, Minus, TriangleAlert, Clock3, Coffee,
  FilePenLine, FilePlus2, FileText, FolderKanban, GitBranch, Grid3X3, History,
  Link2, ListChecks, ListTree, Map, MessageSquareText, MousePointer2, Network, Play,
  Puzzle, RadioTower, ScrollText, Search, Send, Sparkles, SquarePlus, Sunrise, Terminal, Users,
} from "lucide-react";
import type { ToolKindVisual } from "./toolKindVisual";

const ACTION_VISUALS: Record<string, ToolKindVisual> = {
  "plan create": { glyph: Map, color: toolTones.info, label: "plan" },
  "tab get-text": { glyph: ScrollText, color: toolTones.info, label: "tab output" },
  "tab new": { glyph: SquarePlus, color: toolTones.info, label: "new tab" },
  "tab list": { glyph: ListTree, color: toolTones.info, label: "tab list" },
  "tab focus": { glyph: MousePointer2, color: toolTones.info, label: "focus tab" },
  "tab exec": { glyph: Play, color: toolTones.success, label: "tab exec" },
  "tab close": { glyph: LogOut, color: toolTones.info, label: "close tab" },
  "tab search": { glyph: Search, color: toolTones.info, label: "tab search" },
  "tab status": { glyph: Activity, color: toolTones.success, label: "tab status" },
  "workspace focus": { glyph: FolderKanban, color: toolTones.info, label: "workspace" },
  "plan goal set": { glyph: GitBranch, color: toolTones.info, label: "goal" },
  "plan section add": { glyph: FilePlus2, color: toolTones.info, label: "section" },
  "plan section update": { glyph: FilePenLine, color: toolTones.info, label: "section edit" },
  "plan task add": { glyph: ListChecks, color: toolTones.info, label: "plan task" },
  "plan task update": { glyph: FilePenLine, color: toolTones.info, label: "task edit" },
  "plan task close": { glyph: ListChecks, color: toolTones.success, label: "close task" },
  "plan retrospective": { glyph: History, color: toolTones.info, label: "retrospective" },
  "plan record": { glyph: MessageSquareText, color: toolTones.info, label: "decision" },
  "plan skill new": { glyph: Sparkles, color: toolTones.info, label: "skill" },
  "plan state": { glyph: Activity, color: toolTones.success, label: "state" },
  "spec create": { glyph: FilePlus2, color: toolTones.info, label: "spec" },
  "spec section add": { glyph: FileText, color: toolTones.info, label: "spec section" },
  "spec link add": { glyph: Link2, color: toolTones.info, label: "spec link" },
  "agent spawn": { glyph: Bot, color: toolTones.info, label: "spawn" },
  "orchestrator status": { glyph: Users, color: toolTones.info, label: "agents" },
  "agent report": { glyph: MessageSquareText, color: toolTones.info, label: "report" },
  "mesh peers": { glyph: Network, color: toolTones.info, label: "peers" },
  "mem save": { glyph: Brain, color: toolTones.warning, label: "mem save" },
  "mem search": { glyph: Search, color: toolTones.warning, label: "mem search" },
  "mem wake-up": { glyph: Sunrise, color: toolTones.warning, label: "wake-up" },
  "mem get": { glyph: BookOpen, color: toolTones.warning, label: "mem get" },
  "cron create": { glyph: Clock3, color: toolTones.info, label: "cron" },
  "cron list": { glyph: CalendarClock, color: toolTones.info, label: "crons" },
  "keepawake start": { glyph: Coffee, color: toolTones.success, label: "keepawake" },
  "keepawake status": { glyph: Coffee, color: toolTones.success, label: "status" },
  docs: { glyph: BookOpen, color: toolTones.muted, label: "docs" },
  "plugin list": { glyph: Puzzle, color: toolTones.warning, label: "plugins" },
  "plan approve": { glyph: CheckCircle2, color: toolTones.success, label: "approved" },
};

export function actionVisual(invocation: CodetermInvocation): ToolKindVisual {
  const { path } = invocation;
  const command = path.join(" ");
  if (isHelp(invocation)) {
    return { glyph: BookOpen, color: toolTones.muted, label: "help" };
  }
  if (command === "send" && typeof invocation.flags.mesh === "string") {
    return { glyph: RadioTower, color: toolTones.info, label: "mesh send" };
  }
  if (ACTION_VISUALS[command]) return ACTION_VISUALS[command];
  const family = path[0];
  if (family === "notify") return { glyph: Bell, color: toolTones.info, label: "notify" };
  if (family === "send") return { glyph: Send, color: toolTones.info, label: "send" };
  if (family === "plan") return { glyph: Map, color: toolTones.info, label: "plan" };
  if (family === "spec") return { glyph: FileText, color: toolTones.info, label: "spec" };
  if (family === "agent") return { glyph: Bot, color: toolTones.info, label: "agent" };
  if (family === "mem") return { glyph: Brain, color: toolTones.warning, label: "mem" };
  if (family === "mesh") return { glyph: Network, color: toolTones.info, label: "mesh" };
  if (family === "tab") return { glyph: Grid3X3, color: toolTones.info, label: path.join(" ") };
  if (family === "workspace") return { glyph: Grid3X3, color: toolTones.muted, label: "workspace" };
  if (family === "cron" || family === "keepawake") return { glyph: Clock3, color: toolTones.info, label: family };
  if (family === "docs") return { glyph: BookOpen, color: toolTones.muted, label: "docs" };
  if (family === "plugin") return { glyph: Puzzle, color: toolTones.warning, label: "plugin" };
  return { glyph: Terminal, color: toolTones.info, label: "codeterm" };
}

export const toolOutcomeVisual: Record<ToolEvent["status"], ToolKindVisual> = {
  success: { glyph: Check, color: toolTones.success, label: "success" },
  error: { glyph: CircleX, color: toolTones.danger, label: "error" },
  pending: { glyph: LoaderCircle, color: toolTones.info, label: "pending" },
  unresolved: { glyph: Minus, color: toolTones.muted, label: "no result" },
  interrupted: { glyph: TriangleAlert, color: toolTones.warning, label: "interrupted" },
};

export function toolEventVisual(event: ToolEvent): ToolKindVisual {
  let visual = toolKindVisual[event.kind];
  if (event.kind === "command" || event.kind === "codeterm") {
    const command = toolCommand(event);
    const shape = commandShape(command.command, command.argv);
    if (shape.kind === "codeterm") visual = actionVisual(shape.invocation);
    else if (shape.kind === "read" || shape.kind === "search") visual = toolKindVisual[shape.kind];
  }
  return toolVisualForOutcome(visual, event.status);
}

export function toolVisualForOutcome(visual: ToolKindVisual, status: ToolEvent["status"]): ToolKindVisual {
  return status === "error" || status === "interrupted" || status === "unresolved"
    ? { ...visual, color: toolOutcomeVisual[status].color }
    : visual;
}

export function toolStepVisual(event: ToolEvent): ToolKindVisual {
  const kind = toolEventVisual(event);
  const outcome = toolOutcomeVisual[event.status];
  return { ...(event.status === "success" ? kind : outcome), label: `${kind.label}, ${outcome.label}` };
}
