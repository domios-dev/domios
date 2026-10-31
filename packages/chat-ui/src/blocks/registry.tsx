import React from "react";
import { parseDsBlocks, serializeDsBlock, type DsBlock } from "@codeterm/chat-core";
import { GuideDirective } from "../GuideDirective";
import { ConnectBlock } from "./ConnectBlock";
import { PluginBlock } from "./PluginBlock";
import { SettingBlock } from "./SettingBlock";
import { AgentBlock } from "./AgentBlock";
import { ChecklistBlock } from "./ChecklistBlock";
import { ChoiceBlock } from "./ChoiceBlock";
import { JobBlock } from "./JobBlock";
import { TasksBlock } from "./TasksBlock";
import { DS_TASK_GROUP_ALIASES } from "../codeterm/dsTasksBlock";

export const CHAT_BLOCKS = ["guide", "plugin", "connect", "setting", "choice", "agent", "job", "checklist", "tasks"] as const;
export interface ParsedChatBlock { name: DsBlock["kind"]; source: string; render(): React.ReactNode }

export function renderDsCard(block: DsBlock): React.ReactNode {
  const source = serializeDsBlock(block);
  switch (block.kind) {
    case "guide": return <GuideDirective sectionId={block.section} />;
    case "plugin": return <PluginBlock pluginId={block.id} source={source} />;
    case "connect": return <ConnectBlock bridgeId={block.bridge} source={source} />;
    case "setting": return <SettingBlock settingKey={block.key} source={source} />;
    case "choice": return <ChoiceBlock options={block.options} source={source} />;
    case "agent": return <AgentBlock tabRef={block.tab} source={source} />;
    case "job": return <JobBlock jobRef={block.ref} source={source} />;
    case "checklist": return <ChecklistBlock checklistId={block.id} source={source} renderBlock={renderNestedBlock} />;
    case "tasks": return <TasksBlock spec={{ planId: block.plan, refs: block.refs, groups: [...new Set(block.statuses.flatMap((status) => DS_TASK_GROUP_ALIASES[status] ?? []))], items: block.snapshot?.items ?? null, planTitle: block.snapshot?.title }} source={source} />;
    default: return null;
  }
}
function renderNestedBlock(source: string): React.ReactNode { return parseChatBlock(source)?.render() ?? null; }
export function parseChatBlock(source: string): ParsedChatBlock | null {
  const segments = parseDsBlocks(source);
  const blocks = segments.filter((segment) => segment.type !== "markdown" || segment.text.trim());
  if (blocks.length !== 1 || blocks[0].type !== "block") return null;
  const block = blocks[0].block;
  if (!(CHAT_BLOCKS as readonly string[]).includes(block.kind)) return null;
  return { name: block.kind, source: serializeDsBlock(block), render: () => renderDsCard(block) };
}
export function parseChatBlocks(source: string): ParsedChatBlock[] | null {
  const segments = parseDsBlocks(source).filter((segment) => segment.type !== "markdown" || segment.text.trim());
  if (!segments.length || segments.some((segment) => segment.type !== "block" || !(CHAT_BLOCKS as readonly string[]).includes(segment.block.kind))) return null;
  return segments.flatMap((segment) => segment.type === "block" ? [{ name: segment.block.kind, source: serializeDsBlock(segment.block), render: () => renderDsCard(segment.block) }] : []);
}
