import React from "react";
import { describe, expect, it } from "vitest";
import { reduceToolEvents, type NormalizedChatMessage, type ToolEvent } from "@codeterm/chat-core";
import { ToolCallBubble } from "../ToolCallBubble";
import { CodetermActionCard } from "../toolcard/CodetermActionCard";
import { render } from "../toolcard/cardTestSetup";
import { recognize } from "./recognize";
import { describePluginAction } from "./pluginAction";
import { COMMAND_PATHS } from "./commandPaths";
import { describeAction } from "./templates";
import { PluginNameProvider } from "./PluginNames";

const NAMES: Record<string, string> = { lmstudio: "LM Studio" };
const resolve = (id: string) => NAMES[id];

function inv(command: string) {
  const invocation = recognize(command);
  if (!invocation) throw new Error(`not a codeterm command: ${command}`);
  return invocation;
}

function chainEvent(command: string): ToolEvent {
  const session = { sourceSessionId: "plugin-session", provider: "codex" };
  const messages: NormalizedChatMessage[] = [
    { id: "chain", type: "tool_call", content: "", toolName: "exec_command", toolId: "chain", toolKind: "command", toolSummary: command, toolDetail: command, toolInput: { cmd: command }, ...session },
    { id: "chain:result", type: "tool_result", content: "", toolResult: "ok", toolId: "chain", toolExitCode: 0, ...session },
  ];
  const [item] = reduceToolEvents(messages);
  if (item?.kind !== "tool") throw new Error("expected one tool event");
  return item.event;
}

function singleEvent(command: string): ToolEvent {
  return { id: "single", phase: "result", status: "success", kind: "codeterm", summary: command, detail: command, message: { id: "single", type: "tool_call", content: "" } };
}

describe("plugin command rows", () => {
  it("name a resolved plugin by its display name and an unknown one by its id", () => {
    expect(describePluginAction(inv("codeterm plugin get lmstudio"), resolve).object).toBe("LM Studio");
    expect(describePluginAction(inv("codeterm plugin config bitwarden"), resolve).object).toBe("bitwarden");
  });

  it("show what an agent verb call asked the plugin to do", () => {
    const names = (id: string) => ({ browser: "Browser" })[id];
    expect(describePluginAction(inv("codeterm plugin browser goto https://google.com/"), names).object).toBe("goto google.com");
    expect(describePluginAction(inv("codeterm plugin browser snapshot"), names).object).toBe("snapshot");
    expect(describePluginAction(inv('codeterm plugin browser type @e10 "привет мир"'), names).object).toBe('type @e10 "привет мир"');
    expect(describePluginAction(inv('codeterm plugin browser wait --text "Saved"'), names).object).toBe("wait --text Saved");
    expect(describePluginAction(inv("codeterm plugin browser screenshot --full"), names).object).toBe("screenshot --full");
    expect(describePluginAction(inv("codeterm plugin browser snapshot"), names).lead).toBe("Browser");
    expect(describePluginAction(inv("codeterm plugin browser"), names).object).toBe("Browser");
  });

  it("name a folder install by the folder, never the absolute path", () => {
    const row = describePluginAction(inv("codeterm plugin install --from-dir C:\\Users\\me\\codeterm-plugins\\plugins\\transcriber"), resolve);
    expect(row.object).toBe("transcriber");
  });

  it("describe a bare update as every plugin", () => {
    expect(describePluginAction(inv("codeterm plugin update"), resolve).object).toBe("all plugins");
  });

  it("never print the CLI path as the row lead", () => {
    for (const command of ["codeterm plugin get lmstudio", "codeterm plugin config lmstudio", "codeterm plugin remove lmstudio"]) {
      const { lead, object } = describePluginAction(inv(command), resolve);
      expect(`${lead} ${object}`).not.toContain(inv(command).path.join(" "));
    }
  });

  it("every plugin subcommand that targets a plugin shows its display name, never the CLI path", () => {
    const idless = new Set(["list", "update-system"]);
    const targeted = Object.entries(COMMAND_PATHS.plugin).filter(([sub, children]) => Object.keys(children).length === 0 && !idless.has(sub));
    expect(targeted.length).toBeGreaterThan(0);
    for (const [sub] of targeted) {
      const { lead, object } = describeAction(inv(`codeterm plugin ${sub} lmstudio`), undefined, undefined, undefined, resolve);
      expect(object, sub).toBe("LM Studio");
      expect(lead, sub).not.toBe(`plugin ${sub}`);
    }
  });

  it("render display names in compact tree lines under the provider", async () => {
    const host = await render(
      <PluginNameProvider resolve={resolve}>
        <ToolCallBubble event={chainEvent("codeterm plugin get lmstudio ; codeterm plugin config lmstudio")} expanded={false} onToggle={() => {}} />
      </PluginNameProvider>,
    );
    const objects = [...host.querySelectorAll('[data-testid="chat-command-tree-object"]')].map((node) => node.textContent);
    expect(objects).toEqual(["LM Studio", "LM Studio"]);
  });

  it("render the display name as the action card summary under the provider", async () => {
    const host = await render(
      <PluginNameProvider resolve={resolve}>
        <CodetermActionCard event={singleEvent("codeterm plugin get lmstudio")} expanded={false} onToggle={() => {}} />
      </PluginNameProvider>,
    );
    expect(host.textContent).toContain("LM Studio");
    expect(host.textContent).not.toContain("plugin get lmstudio");
  });
});
