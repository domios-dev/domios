import React from "react";
import { CommandCard } from "./CommandCard";
import { CodetermActionCard } from "./CodetermActionCard";
import { event, render } from "./cardTestSetup";
import { ShowCommandsProvider } from "./toolKindVisual";

const fullCommand = "git log --oneline -2; git status --short; git diff --stat";
const longCommand = Array.from({ length: 100 }, (_, index) => `echo command-${index}`).join("\n");
const withCommandsOff = (card: React.ReactElement) => <ShowCommandsProvider showCommands={false}>{card}</ShowCommandsProvider>;

const commandHost = await render(withCommandsOff(<CommandCard event={event({ kind: "command", detail: fullCommand })} expanded onToggle={() => {}} />));
if (commandHost.querySelector('[data-testid="toolcard-command"]')?.textContent !== fullCommand) throw new Error("command_card_reveal expanded command card must render the full command");
if (commandHost.querySelector('[aria-label="Hide full command"]')?.getAttribute("aria-expanded") !== "true") throw new Error("command_card_reveal expanded command must expose a meaningful disclosure name");
if (commandHost.querySelector('[role="region"][aria-label="Full command text"]')?.getAttribute("tabindex") !== "0") throw new Error("command_card_reveal full command must be keyboard reachable");

const codetermHost = await render(withCommandsOff(<CodetermActionCard event={event({ kind: "codeterm", detail: fullCommand, summary: fullCommand })} expanded onToggle={() => {}} />));
if (codetermHost.querySelector('[data-testid="toolcard-command"]')?.textContent?.includes(fullCommand) !== true) throw new Error("command_card_reveal expanded CodeTerm card must render the full command");
if (!codetermHost.querySelector('[aria-label="Hide full command"]')) throw new Error("command_card_reveal CodeTerm command must expose a meaningful disclosure name");
if (codetermHost.querySelector('[role="region"][aria-label="Full command text"]')?.getAttribute("tabindex") !== "0") throw new Error("command_card_reveal CodeTerm command must be keyboard reachable");

for (const [name, host] of [
  ["command", await render(withCommandsOff(<CommandCard event={event({ kind: "command", detail: longCommand })} expanded onToggle={() => {}} />))],
  ["CodeTerm", await render(withCommandsOff(<CodetermActionCard event={event({ kind: "codeterm", detail: longCommand, summary: longCommand })} expanded onToggle={() => {}} />))],
] as const) {
  const command = host.querySelector<HTMLElement>('[data-testid="toolcard-command"]');
  if (!command?.classList.contains("overflow-auto") || command.style.maxHeight !== "12rem") throw new Error(`command_card_reveal ${name} command must stay bounded and scrollable`);
}

for (const [name, host] of [
  ["command", await render(withCommandsOff(<CommandCard event={event({ kind: "command", detail: fullCommand })} expanded={false} onToggle={() => {}} />))],
  ["CodeTerm", await render(withCommandsOff(<CodetermActionCard event={event({ kind: "codeterm", detail: fullCommand, summary: fullCommand })} expanded={false} onToggle={() => {}} />))],
] as const) {
  if (host.querySelector('[data-testid="toolcard-command"]')) throw new Error(`command_card_reveal collapsed ${name} card must keep the command hidden`);
  if (!host.querySelector('[aria-label="Show full command"]')) throw new Error(`command_card_reveal collapsed ${name} command must expose a meaningful disclosure name`);
}

const privacyHost = await render(withCommandsOff(<CommandCard event={event({ kind: "command", detail: fullCommand, message: { id: "id", type: "tool_call", content: '{"raw":"tool JSON"}' } })} expanded onToggle={() => {}} />));
if (privacyHost.textContent?.includes('{"raw')) throw new Error("command_card_reveal raw tool JSON must remain hidden");

console.log("command_card_reveal tests: passed");
