export { isolateBlockDirectives } from "../blockDirectives";
import React from "react";
import { GuideDirective, parseGuideDirective } from "../GuideDirective";
import { ConnectBlock } from "./ConnectBlock";
import { parseBridgeId } from "./connectBlockModel";
import { PluginBlock } from "./PluginBlock";
import { parsePluginId } from "./pluginBlockModel";
import { SettingBlock } from "./SettingBlock";
import { parseSettingKey } from "./settingBlockModel";
import { AgentBlock } from "./AgentBlock";
import { parseTabRef } from "./agentBlockModel";
import { ChecklistBlock } from "./ChecklistBlock";
import { parseChecklistId } from "./checklistBlockModel";
import { ChoiceBlock } from "./ChoiceBlock";
import { parseChoiceOptions } from "./choiceBlockModel";
import { JobBlock } from "./JobBlock";
import { parseJobRef } from "./jobBlockModel";
import { TasksBlock } from "./TasksBlock";
import { parseTasksArgument, type TasksBlockSpec } from "./tasksBlockModel";

/** One `@ds-<name> <argument>` chat block: a parser that validates the argument and a renderer for it. */
export interface ChatBlockDefinition<P> {
  name: string;
  parse(argument: string, line: string): P | null;
  render(props: P, source: string): React.ReactNode;
}

export interface ParsedChatBlock {
  name: string;
  source: string;
  render(): React.ReactNode;
}

function defineChatBlock<P>(definition: ChatBlockDefinition<P>): ChatBlockDefinition<unknown> {
  return definition as ChatBlockDefinition<unknown>;
}

/** Every block a reply can emit. Add a block here with its parser and renderer, then list it in prod-docs/CHAT-BLOCKS.md. */
export const CHAT_BLOCKS: readonly ChatBlockDefinition<unknown>[] = [
  defineChatBlock<string>({
    name: "guide",
    parse: (_argument, line) => parseGuideDirective(line),
    render: (sectionId) => <GuideDirective sectionId={sectionId} />,
  }),
  defineChatBlock<string>({
    name: "plugin",
    parse: (argument) => parsePluginId(argument),
    render: (pluginId, source) => <PluginBlock pluginId={pluginId} source={source} />,
  }),
  defineChatBlock<string>({
    name: "connect",
    parse: (argument) => parseBridgeId(argument),
    render: (bridgeId, source) => <ConnectBlock bridgeId={bridgeId} source={source} />,
  }),
  defineChatBlock<string>({
    name: "setting",
    parse: (argument) => parseSettingKey(argument),
    render: (settingKey, source) => <SettingBlock settingKey={settingKey} source={source} />,
  }),
  defineChatBlock<string[]>({
    name: "choice",
    parse: (argument) => parseChoiceOptions(argument),
    render: (options, source) => <ChoiceBlock options={options} source={source} />,
  }),
  defineChatBlock<string>({
    name: "agent",
    parse: (argument) => parseTabRef(argument),
    render: (tabRef, source) => <AgentBlock tabRef={tabRef} source={source} />,
  }),
  defineChatBlock<string>({
    name: "job",
    parse: (argument) => parseJobRef(argument),
    render: (jobRef, source) => <JobBlock jobRef={jobRef} source={source} />,
  }),
  defineChatBlock<string>({
    name: "checklist",
    parse: (argument) => parseChecklistId(argument),
    render: (checklistId, source) => <ChecklistBlock checklistId={checklistId} source={source} renderBlock={renderNestedBlock} />,
  }),
  defineChatBlock<TasksBlockSpec>({
    name: "tasks",
    parse: (argument) => parseTasksArgument(argument),
    render: (spec, source) => <TasksBlock spec={spec} source={source} />,
  }),
];

function renderNestedBlock(source: string): React.ReactNode {
  return parseChatBlock(source)?.render() ?? null;
}

const BLOCK_LINE = /^[\t ]*@ds-([a-z][a-z0-9-]*)[\t ]+(\S(?:.*\S)?)[\t ]*$/;

/** Parses a standalone one-line block directive; anything else (inline, multi-line, unknown name) stays text. */
export function parseChatBlock(source: string, blocks: readonly ChatBlockDefinition<unknown>[] = CHAT_BLOCKS): ParsedChatBlock | null {
  const line = source.replace(/\r/g, "");
  const match = BLOCK_LINE.exec(line);
  if (!match) return null;
  const [, name, argument] = match;
  const definition = blocks.find((block) => block.name === name);
  if (!definition) return null;
  const props = definition.parse(argument, line);
  if (props === null) return null;
  const normalized = `@ds-${name} ${argument}`;
  return { name, source: normalized, render: () => definition.render(props, normalized) };
}

/** A paragraph made only of block lines renders as stacked blocks; one stray text line keeps the whole paragraph text. */
export function parseChatBlocks(source: string, blocks: readonly ChatBlockDefinition<unknown>[] = CHAT_BLOCKS): ParsedChatBlock[] | null {
  const lines = source.replace(/\r/g, "").split("\n").filter((line) => line.trim());
  if (!lines.length) return null;
  const parsed = lines.map((line) => parseChatBlock(line, blocks));
  return parsed.every((block): block is ParsedChatBlock => block !== null) ? parsed : null;
}
