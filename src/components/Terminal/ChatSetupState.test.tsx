// Run: npx tsx src/components/Terminal/ChatSetupState.test.tsx

import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import {
  ChatSetupState,
  ChatSetupStatusLine,
  chatSetupSubject,
  chatSetupStageLabel,
  chatSetupView,
} from "./ChatSetupState";
import type { Pane } from "../../lib/types";

const win = new Window({ url: "http://localhost/chat-setup" });
Object.defineProperty(globalThis, "window", { value: win, configurable: true });
Object.defineProperty(globalThis, "document", { value: win.document, configurable: true });
Object.defineProperty(globalThis, "HTMLElement", { value: win.HTMLElement, configurable: true });
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let failures = 0;
async function test(name: string, fn: () => void | Promise<void>) {
  try {
    await fn();
    console.log(`  ✓ ${name}`);
  } catch (e) {
    failures++;
    console.error(`  ✗ ${name}: ${e instanceof Error ? e.message : String(e)}`);
  }
}

function assert(cond: unknown, msg: string): asserts cond {
  if (!cond) throw new Error(msg);
}

function pane(stage: Pane["boot_stage"], reason?: string | null): Pane {
  return {
    id: "p-1",
    pty_id: null,
    title: "Agent",
    boot_stage: stage,
    boot_failure_reason: reason ?? null,
  } as unknown as Pane;
}

async function render(element: React.ReactElement): Promise<HTMLElement> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(element);
  });
  return host;
}

async function main() {
  await test("stage label maps pty stages to terminal prep and launch stages to the provider", () => {
    assert(chatSetupStageLabel("pty_starting", "Claude Code") === "Preparing Claude Code", "pty_starting");
    assert(chatSetupStageLabel("pty_spawned", "Claude Code") === "Preparing Claude Code", "pty_spawned");
    assert(chatSetupStageLabel("shell_ready", "Claude Code") === "Starting Claude Code", "shell_ready");
    assert(chatSetupStageLabel("agent_launching", "Claude Code") === "Starting Claude Code", "agent_launching");
  });

  await test("role identity takes precedence over provider during launch", () => {
    const orchestrator = { ...pane("agent_launching"), role: "Orchestrator" } as Pane;
    const planner = { ...pane("agent_launching"), role: "Manager", role_profile: "planner" } as Pane;
    assert(chatSetupSubject(orchestrator, "Codex") === "Orchestrator", "orchestrator label");
    assert(chatSetupSubject(planner, "Claude Code") === "Planner", "planner label");
  });

  await test("view resolves ready to none and launching to starting", () => {
    assert(chatSetupView(pane("agent_ready")).kind === "none", "ready is none");
    assert(chatSetupView(pane("agent_launching")).kind === "starting", "launching is starting");
    assert(chatSetupView(pane("spawn_failed", "boom")).kind === "failed", "spawn_failed is failed");
    assert(chatSetupView(null).kind === "none", "no pane is none");
  });

  await test("starting renders the setup card with the stage label", async () => {
    const p = pane("agent_launching");
    const host = await render(
      <ChatSetupState pane={p} view={chatSetupView(p)} providerId="claude" providerName="Claude Code" />,
    );
    assert(host.querySelector('[data-testid="chat-setup-state"]'), "setup card rendered");
    const stage = host.querySelector('[data-testid="chat-setup-stage"]');
    assert(stage?.textContent?.includes("Starting Claude Code"), "stage label shows the provider");
  });

  await test("history can keep rendering while the compact boot status remains visible", async () => {
    const host = await render(
      <ChatSetupStatusLine pane={pane("pty_spawned")} providerName="Codex" />,
    );
    const status = host.querySelector('[data-testid="chat-setup-status-line"]');
    assert(status, "compact status rendered");
    assert(status.textContent?.includes("Preparing Codex"), "real boot stage is shown");
  });

  await test("compact boot status disappears when the provider is ready", async () => {
    const host = await render(
      <ChatSetupStatusLine pane={pane("agent_ready")} providerName="Codex" />,
    );
    assert(!host.querySelector('[data-testid="chat-setup-status-line"]'), "ready status hidden");
  });

  await test("failed renders the reason and an Inspect-in-Terminal action", async () => {
    const p = pane("spawn_failed", "pty is closed");
    let opened = false;
    const host = await render(
      <ChatSetupState
        pane={p}
        view={chatSetupView(p)}
        providerId="claude"
        providerName="Claude Code"
        onOpenTerminal={() => {
          opened = true;
        }}
      />,
    );
    const card = host.querySelector('[data-testid="chat-launch-failed"]');
    assert(card, "failed card rendered");
    assert(card.textContent?.includes("Claude Code failed to launch"), "names the provider");
    assert(card.textContent?.includes("pty is closed"), "shows the reason");
    const btn = host.querySelector('[data-testid="chat-launch-failed-open-terminal"]') as HTMLButtonElement | null;
    assert(btn, "inspect button present");
    await act(async () => {
      btn.click();
    });
    assert(opened, "inspect button opens the terminal");
  });

  await test("ready renders nothing (empty-state text stays the caller's job)", async () => {
    const p = pane("agent_ready");
    const host = await render(
      <ChatSetupState pane={p} view={chatSetupView(p)} providerId="claude" providerName="Claude Code" />,
    );
    assert(!host.querySelector('[data-testid="chat-setup-state"]'), "no setup card");
    assert(!host.querySelector('[data-testid="chat-launch-failed"]'), "no failed card");
  });

  console.log(failures === 0 ? "ChatSetupState tests: all passed" : `ChatSetupState tests: ${failures} failed`);
  if (failures > 0) process.exit(1);
}

void main();
