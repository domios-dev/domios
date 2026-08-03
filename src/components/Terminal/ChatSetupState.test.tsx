// Run: npx tsx src/components/Terminal/ChatSetupState.test.tsx

import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import {
  ChatSetupState,
  chatSetupSubject,
  chatSetupStageLabel,
  chatSetupView,
} from "./ChatSetupState";
import type { Tab } from "../../lib/types";

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

function tab(stage: Tab["boot_stage"], reason?: string | null, protectedTab = true): Tab {
  return {
    id: "p-1",
    pty_id: null,
    title: "Agent",
    boot_stage: stage,
    boot_failure_reason: reason ?? null,
    protected: protectedTab,
  } as unknown as Tab;
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
  await test("stage labels are launch-event labels", () => {
    assert(chatSetupStageLabel("pty_starting", "Claude Code") === "Creating tab…", "pty_starting");
    assert(chatSetupStageLabel("pty_spawned", "Claude Code") === "Spawn accepted…", "pty_spawned");
    assert(chatSetupStageLabel("shell_ready", "Claude Code") === "Session attached…", "shell_ready");
    assert(chatSetupStageLabel("preparing_role_instructions", "Claude Code") === "Preparing claude code instructions…", "preparing_role_instructions");
    assert(chatSetupStageLabel("agent_launching", "Claude Code") === "Launching provider…", "agent_launching");
    assert(chatSetupStageLabel("delivering_instructions", "Claude Code") === "Delivering instructions…", "delivering_instructions");
  });

  await test("role identity takes precedence over provider during launch", () => {
    const orchestrator = { ...tab("agent_launching"), role: "Orchestrator" } as Tab;
    const planner = { ...tab("agent_launching"), role: "Manager", role_profile: "planner" } as Tab;
    assert(chatSetupSubject(orchestrator, "Codex") === "Orchestrator", "orchestrator label");
    assert(chatSetupSubject(planner, "Claude Code") === "Planner", "planner label");
  });

  await test("view resolves ready to none and launching to starting", () => {
    assert(chatSetupView(tab("agent_ready")).kind === "none", "ready is none");
    assert(chatSetupView(tab("preparing_role_instructions")).kind === "starting", "preparing is starting");
    assert(chatSetupView(tab("agent_launching")).kind === "starting", "launching is starting");
    assert(chatSetupView(tab("delivering_instructions")).kind === "starting", "fallback delivery is starting");
    assert(chatSetupView(tab("spawn_failed", "boom")).kind === "starting", "spawn_failed keeps loading");
    assert(chatSetupView(tab("pty_reattached")).kind === "none", "a reattached live PTY is not booting");
    assert(chatSetupView(null).kind === "none", "no Tab is none");
  });

  await test("an authoritative attached chat suppresses stale setup stages", () => {
    const reserved = { ...tab(null), agent_booting: true } as Tab;
    assert(chatSetupView(reserved, true).kind === "none", "reserved state is stale after hydration");
    assert(chatSetupView(tab("pty_starting"), true).kind === "none", "early launch state is stale after hydration");
    assert(chatSetupView(tab("shell_ready"), true).kind === "none", "ShellReady is stale after hydration");
  });

  await test("starting renders only the provider progress glyph", async () => {
    const p = { ...tab("preparing_role_instructions"), cwd: "/work/identity-card", role: "Worker", agent_session_model: "Focused" } as Tab;
    const host = await render(
      <ChatSetupState tab={p} view={chatSetupView(p)} providerName="Claude Code" providerIconSvg="<svg><path /></svg>" providerIconColor="#d97757" />,
    );
    assert(host.querySelector('[data-testid="chat-setup-state"]'), "setup card rendered");
    assert(host.querySelector('[data-testid="chat-launch-glyph"]')?.getAttribute("aria-label") === "Claude Code", "glyph is named by descriptor display name");
    assert(host.querySelector('[data-testid="chat-launch-ring"]'), "thin conic ring rendered");
    assert(host.querySelectorAll('[data-testid="chat-launch-ticks"] i').length === 8, "static reduced-motion tick marks are present");
  });

  await test("failed launch remains a neutral provider loader", async () => {
    const p = tab("spawn_failed", "SYSTEM REMINDER: secret launch details");
    const host = await render(
      <ChatSetupState tab={p} view={chatSetupView(p)} providerName="Claude Code" />,
    );
    assert(host.querySelector('[data-testid="chat-setup-state"]'), "neutral setup loader remains rendered");
    assert(host.querySelector('[data-testid="chat-launch-ring"]'), "progress ring remains rendered");
    assert(!host.querySelector('[data-testid="chat-launch-failed"]'), "no failure card is rendered");
    assert(!host.textContent?.includes("could not"), "no failure copy is rendered");
    assert(!host.textContent?.includes("SYSTEM REMINDER"), "raw failure detail is hidden");
  });

  await test("missing provider art uses a contained diamond fallback", async () => {
    const host = await render(
      <ChatSetupState tab={tab("agent_launching")} view={chatSetupView(tab("agent_launching"))} providerName="Codex" providerIconHint="</>" />,
    );
    const glyph = host.querySelector(".chat-launch-glyph .agent-icon");
    assert(glyph?.textContent === "◆", "fallback is a single diamond");
    assert(glyph?.classList.contains("text-[18px]"), "fallback stays inside the launch ring");
  });

  await test("ready removes the setup overlay in the transcript commit", async () => {
    const host = document.createElement("div");
    document.body.appendChild(host);
    const root = createRoot(host);
    const starting = tab("agent_launching");
    const ready = tab("agent_ready");
    const renderState = async (current: Tab) => {
      await act(async () => {
        root.render(<><div data-testid="incoming-transcript">Transcript</div><ChatSetupState tab={current} view={chatSetupView(current)} providerName="Codex" /></>);
      });
    };
    await renderState(starting);
    await renderState(ready);
    assert(!host.querySelector('[data-testid="chat-setup-state"]'), "ready unmounts setup immediately");
    assert(host.querySelector('[data-testid="incoming-transcript"]'), "transcript remains visible");
  });

  await test("ready renders nothing (empty-state text stays the caller's job)", async () => {
    const p = tab("agent_ready");
    const host = await render(
      <ChatSetupState tab={p} view={chatSetupView(p)} providerName="Claude Code" />,
    );
    assert(!host.querySelector('[data-testid="chat-setup-state"]'), "no setup card");
    assert(!host.querySelector('[data-testid="chat-launch-failed"]'), "no failed card");
  });

  await test("an attached chat is authoritative over a late failed boot stage", async () => {
    const p = tab("spawn_failed", "stale launch result");
    const host = await render(
      <ChatSetupState tab={p} view={chatSetupView(p, true)} providerName="Codex" />,
    );
    assert(!host.querySelector('[data-testid="chat-launch-failed"]'), "attached chat is never covered by a stale failure");
    assert(!host.querySelector('[data-testid="chat-setup-state"]'), "attached chat has no launch overlay");
  });

  console.log(failures === 0 ? "ChatSetupState tests: all passed" : `ChatSetupState tests: ${failures} failed`);
  if (failures > 0) process.exit(1);
}

void main();
