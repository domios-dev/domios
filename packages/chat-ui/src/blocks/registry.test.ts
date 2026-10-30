import assert from "node:assert/strict";
import test from "node:test";
import { parseChatBlock, parseChatBlocks, CHAT_BLOCKS } from "./registry";
import { parsePluginId } from "./pluginBlockModel";
import { parseDsBlocks } from "@codeterm/chat-core";

test("all nine typed live cards use the shared grammar", () => {
  const tags = ['<ds-guide section="phone" />', '<ds-plugin id="git" />', '<ds-connect bridge="telegram" />', '<ds-setting key="plugins.git.enabled" />', '<ds-agent tab="e3ec9ee9" />', '<ds-job ref="nightly" />', '<ds-checklist id="getting-started" />', '<ds-choice options=\'["Yes","No"]\' />', '<ds-tasks plan="pl_alpha" />'];
  assert.equal(CHAT_BLOCKS.length, 9);
  assert.equal(parseChatBlocks(tags.join("\r\n"))?.length, 9);
  for (const tag of tags) assert.ok(parseChatBlock(tag));
  assert.equal(parseChatBlocks(tags[0] + "\nprose"), null);
});

test("registry parsing does not render or admit literal contexts and legacy forms", () => {
  const tag = '<ds-plugin id="git" />';
  assert.equal(parseDsBlocks(`Lead\n${tag}\nTail`).filter((part) => part.type === "block").length, 1);
  for (const source of [`see ${tag}`, `\t${tag}`, `> ${tag}`, `\`${tag}\``, `\`\`\`\n${tag}\n\`\`\``, tag + "\nprose", '@ds-plugin git', '@ct-plugin git', '<ds-plugin id="git" extra="x" />']) assert.equal(parseChatBlock(source), null, source);
  assert.equal(parseChatBlock('   <ds-plugin\r\n id="git"\r\n />')?.source, tag);
});

test("plugin domain validation remains strict", () => {
  for (const id of ["telegram-client", "chat-preprocessor", "a", "my_plugin.v2"]) assert.equal(parsePluginId(id), id);
  for (const id of ["../etc", "Telegram", "-lead", "a b", "x..y", "https://example.com/p", "", "a".repeat(65)]) assert.equal(parsePluginId(id), null);
});
