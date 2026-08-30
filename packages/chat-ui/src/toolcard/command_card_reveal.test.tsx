import React, { act } from "react";
import { CommandCard } from "./CommandCard";
import { CodetermActionCard } from "./CodetermActionCard";
import { event, render } from "./cardTestSetup";

const fullCommand = "git log --oneline -2; git status --short; git diff --stat";
const longCommand = Array.from({ length: 100 }, (_, index) => `echo command-${index}`).join("\n");
const commandHost = await render(<CommandCard event={event({ kind: "command", detail: fullCommand, result: "command result", status: "success" })} expanded onToggle={() => {}} />);
if (commandHost.querySelector('[data-testid="message-raw-content"]')) throw new Error("command_card_reveal formatted mode must hide the full command");
if (commandHost.querySelector('[aria-label="Hide full command"]')?.getAttribute("aria-expanded") !== "true") throw new Error("command_card_reveal expanded command must expose a meaningful disclosure name");
const rawButton = Array.from(commandHost.querySelectorAll("button")).find((button) => button.textContent === "Raw");
await act(async () => rawButton?.click());
if (commandHost.querySelector('[data-testid="message-raw-content"]')?.textContent !== fullCommand) throw new Error("command_card_reveal raw mode must render the full command");
if (!commandHost.textContent?.includes("command result")) throw new Error("command_card_reveal raw mode must preserve the result");

const codetermHost = await render(<CodetermActionCard event={event({ kind: "codeterm", detail: fullCommand, summary: fullCommand })} expanded onToggle={() => {}} />);
if (codetermHost.querySelector('[data-testid="message-raw-content"]')) throw new Error("command_card_reveal formatted CodeTerm card must hide the full command");
if (!codetermHost.querySelector('[aria-label="Hide full command"]')) throw new Error("command_card_reveal CodeTerm command must expose a meaningful disclosure name");

const longHost = await render(<CommandCard event={event({ kind: "command", detail: longCommand })} expanded onToggle={() => {}} />);
const longRawButton = Array.from(longHost.querySelectorAll("button")).find((button) => button.textContent === "Raw");
await act(async () => longRawButton?.click());
const longRaw = longHost.querySelector<HTMLElement>('[data-testid="message-raw-content"]');
if (!longRaw?.classList.contains("overflow-auto") || longRaw.textContent !== longCommand) throw new Error("command_card_reveal raw command must stay bounded and complete");

for (const [name, host] of [
  ["command", await render(<CommandCard event={event({ kind: "command", detail: fullCommand })} expanded={false} onToggle={() => {}} />)],
  ["CodeTerm", await render(<CodetermActionCard event={event({ kind: "codeterm", detail: fullCommand, summary: fullCommand })} expanded={false} onToggle={() => {}} />)],
] as const) {
  if (host.querySelector('[data-testid="message-view-toggle"]')) throw new Error(`command_card_reveal collapsed ${name} card must keep the view toggle hidden`);
  if (!host.querySelector('[aria-label="Show full command"]')) throw new Error(`command_card_reveal collapsed ${name} command must expose a meaningful disclosure name`);
}

const privacyHost = await render(<CommandCard event={event({ kind: "command", detail: fullCommand, message: { id: "id", type: "tool_call", content: '{"raw":"tool JSON"}' } })} expanded onToggle={() => {}} />);
if (privacyHost.textContent?.includes('{"raw')) throw new Error("command_card_reveal raw tool JSON must remain hidden");

console.log("command_card_reveal tests: passed");
