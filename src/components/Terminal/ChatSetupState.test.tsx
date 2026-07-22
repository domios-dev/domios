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

function pane(stage: Pane["boot_stage"], reason?: string | null, protectedPane = true): Pane {
  return {
    id: "p-1",
    pty_id: null,
    title: "Agent",
    boot_stage: stage,
    boot_failure_reason: reason ?? null,
    protected: protectedPane,
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
  await test("stage labels are launch-event labels", () => {
    assert(chatSetupStageLabel("pty_starting", "Claude Code") === "Creating pane…", "pty_starting");
    assert(chatSetupStageLabel("pty_spawned", "Claude Code") === "Spawn accepted…", "pty_spawned");
    assert(chatSetupStageLabel("shell_ready", "Claude Code") === "Session attached…", "shell_ready");
    assert(chatSetupStageLabel("preparing_role_instructions", "Claude Code") === "Preparing claude code instructions…", "preparing_role_instructions");
    assert(chatSetupStageLabel("agent_launching", "Claude Code") === "Launching provider…", "agent_launching");
    assert(chatSetupStageLabel("delivering_instructions", "Claude Code") === "Delivering instructions…", "delivering_instructions");
  });

  await test("role identity takes precedence over provider during launch", () => {
    const orchestrator = { ...pane("agent_launching"), role: "Orchestrator" } as Pane;
    const planner = { ...pane("agent_launching"), role: "Manager", role_profile: "planner" } as Pane;
    assert(chatSetupSubject(orchestrator, "Codex") === "Orchestrator", "orchestrator label");
    assert(chatSetupSubject(planner, "Claude Code") === "Planner", "planner label");
  });

  await test("view resolves ready to none and launching to starting", () => {
    assert(chatSetupView(pane("agent_ready")).kind === "none", "ready is none");
    assert(chatSetupView(pane("preparing_role_instructions")).kind === "starting", "preparing is starting");
    assert(chatSetupView(pane("agent_launching")).kind === "starting", "launching is starting");
    assert(chatSetupView(pane("delivering_instructions")).kind === "starting", "fallback delivery is starting");
    assert(chatSetupView(pane("spawn_failed", "boom")).kind === "failed", "spawn_failed is failed");
    assert(chatSetupView(pane("pty_reattached")).kind === "none", "a reattached live PTY is not booting");
    assert(chatSetupView(null).kind === "none", "no pane is none");
  });

  await test("reserved or launching panes stay booting even after chat attaches", () => {
    const reserved = { ...pane(null), agent_booting: true } as Pane;
    assert(chatSetupView(reserved, true).kind === "starting", "reserved pane is booting");
    assert(chatSetupView(pane("pty_starting"), true).kind === "starting", "launching pane is booting");
  });

  await test("starting renders the setup card with the stage label", async () => {
    const p = { ...pane("preparing_role_instructions"), cwd: "/work/identity-card", role: "Worker", agent_session_model: "Focused" } as Pane;
    const host = await render(
      <ChatSetupState pane={p} view={chatSetupView(p)} providerName="Claude Code" providerIconSvg="<svg><path /></svg>" providerIconColor="#d97757" teamName="Launch Crew" projectName="identity-card" />,
    );
    assert(host.querySelector('[data-testid="chat-setup-state"]'), "setup card rendered");
    const stage = host.querySelector('[data-testid="chat-setup-stage"]');
    assert(stage?.textContent?.includes("Preparing worker instructions"), "stage label shows the launch event");
    assert(host.querySelector('[data-testid="chat-launch-glyph"]')?.getAttribute("aria-label") === "Claude Code", "glyph is named by descriptor display name");
    assert(host.querySelector('[data-testid="chat-launch-ring"]'), "thin conic ring rendered");
    assert(host.querySelector('[data-testid="chat-launch-eyebrow"]')?.textContent === "Launch Crew", "actual Team name is rendered");
    assert(host.querySelector('[data-testid="chat-launch-role"]')?.textContent === "Worker", "role line rendered");
    assert(host.querySelector('[data-testid="chat-launch-facts"]')?.textContent === "Claude Code · Focused · identity-card", "honest provider/model/project facts rendered");
    assert(host.querySelectorAll('[data-testid="chat-launch-ticks"] i').length === 8, "static reduced-motion tick marks are present");
    assert(stage?.getAttribute("aria-live") === "polite", "stage is polite live region");
  });

  await test("project identity never exposes absolute paths", async () => {
    const windows = { ...pane("agent_launching"), cwd: "D:\\Developer\\secret-project" } as Pane;
    const posix = { ...pane("agent_launching"), cwd: "/Users/esme/secret-project" } as Pane;
    const windowsHost = await render(<ChatSetupState pane={windows} view={chatSetupView(windows)} providerName="Codex" projectName="Workspace Display" />);
    const posixHost = await render(<ChatSetupState pane={posix} view={chatSetupView(posix)} providerName="Codex" projectName="Workspace Display" />);
    const rendered = `${windowsHost.textContent}\n${posixHost.textContent}`;
    assert(rendered.includes("Workspace Display"), "workspace display name is used");
    assert(!rendered.includes("D:\\Developer\\secret-project"), "Windows path is not rendered");
    assert(!rendered.includes("/Users/esme/secret-project"), "POSIX path is not rendered");
  });

  await test("history can keep rendering while the compact boot status remains visible", async () => {
    const host = await render(
      <ChatSetupStatusLine pane={pane("pty_spawned")} providerName="Codex" />,
    );
    const status = host.querySelector('[data-testid="chat-setup-status-line"]');
    assert(status, "compact status rendered");
    assert(status.textContent?.includes("Spawn accepted"), "real boot stage is shown");
  });

  await test("compact boot status disappears when the provider is ready", async () => {
    const host = await render(
      <ChatSetupStatusLine pane={pane("agent_ready")} providerName="Codex" />,
    );
    assert(!host.querySelector('[data-testid="chat-setup-status-line"]'), "ready status hidden");
  });

  await test("failed renders plain language without raw backend failure text", async () => {
    const p = pane("spawn_failed", "SYSTEM REMINDER: secret launch details");
    let opened = false;
    let retried = false;
    const host = await render(
      <ChatSetupState
        pane={p}
        view={chatSetupView(p)}
        providerName="Claude Code"
        onRetry={() => { retried = true; }}
        onOpenTerminal={() => {
          opened = true;
        }}
      />,
    );
    const card = host.querySelector('[data-testid="chat-launch-failed"]');
    assert(card, "failed card rendered");
    assert(card.textContent?.includes("Claude Code could not start"), "names the provider");
    assert(!card.textContent?.includes("SYSTEM REMINDER"), "raw reminder text is never rendered");
    const btn = host.querySelector('[data-testid="chat-launch-retry"]') as HTMLButtonElement | null;
    assert(btn, "retry button present");
    await act(async () => {
      btn.click();
    });
    assert(retried, "retry routes through launch retry callback");
    assert(!opened, "show log is withheld until a second failure");
  });

  await test("non-retryable failure never offers a retry action", async () => {
    const p = pane("spawn_failed", "not restartable", false);
    const host = await render(
      <ChatSetupState pane={p} view={chatSetupView(p)} providerName="Codex" onRetry={() => {}} />,
    );
    assert(!host.querySelector('[data-testid="chat-launch-retry"]'), "retry is hidden when the pane cannot be restarted");
  });

  await test("a failed provisioned agent offers retry without a provider-specific branch", async () => {
    const p = { ...pane("spawn_failed", "launch failed", false), agent_type: "provider-from-descriptor" } as Pane;
    const host = await render(
      <ChatSetupState pane={p} view={chatSetupView(p)} providerName="Agent" onRetry={() => {}} />,
    );
    assert(host.querySelector('[data-testid="chat-launch-retry"]'), "provisioned agent can retry");
  });

  await test("second launch failure exposes the terminal log action", async () => {
    const host = document.createElement("div");
    document.body.appendChild(host);
    const root = createRoot(host);
    const renderState = async (stage: Pane["boot_stage"]) => {
      const current = pane(stage);
      await act(async () => {
        root.render(<ChatSetupState pane={current} view={chatSetupView(current)} providerName="Claude Code" onRetry={() => {}} onOpenTerminal={() => {}} />);
      });
    };
    await renderState("spawn_failed");
    assert(!host.querySelector('[data-testid="chat-launch-show-log"]'), "first failure does not expose log action");
    await renderState("pty_starting");
    await renderState("spawn_failed");
    assert(host.querySelector('[data-testid="chat-launch-show-log"]'), "second failure exposes log action");
  });

  await test("ready overlaps the transcript only through its opacity transition", async () => {
    const host = document.createElement("div");
    document.body.appendChild(host);
    const root = createRoot(host);
    const starting = pane("agent_launching");
    const ready = pane("agent_ready");
    const renderState = async (current: Pane) => {
      await act(async () => {
        root.render(<><div data-testid="incoming-transcript">Transcript</div><ChatSetupState pane={current} view={chatSetupView(current)} providerName="Codex" /></>);
      });
    };
    await renderState(starting);
    await renderState(ready);
    const exiting = host.querySelector('[data-testid="chat-setup-state"]') as HTMLDivElement | null;
    assert(exiting?.dataset.transitionState === "exiting", "ready begins event-driven exit");
    assert(exiting.style.opacity === "0", "exit changes opacity");
    assert(host.querySelector('[data-testid="incoming-transcript"]'), "transcript is present beneath exiting card");
    const transitionEnd = new window.Event("transitionend", { bubbles: true });
    Object.defineProperty(transitionEnd, "propertyName", { value: "opacity" });
    await act(async () => {
      exiting.dispatchEvent(transitionEnd);
    });
    assert(!host.querySelector('[data-testid="chat-setup-state"]'), "card unmounts after opacity transition");
    assert(host.querySelector('[data-testid="incoming-transcript"]'), "transcript remains after card exits");
  });

  await test("reduced motion removes the card immediately on ready", async () => {
    const matchMediaDescriptor = Object.getOwnPropertyDescriptor(window, "matchMedia");
    Object.defineProperty(window, "matchMedia", {
      configurable: true,
      value: () => ({ matches: true, addEventListener() {}, removeEventListener() {} }),
    });
    try {
      const host = document.createElement("div");
      document.body.appendChild(host);
      const root = createRoot(host);
      const starting = pane("agent_launching");
      const ready = pane("agent_ready");
      await act(async () => {
        root.render(<ChatSetupState pane={starting} view={chatSetupView(starting)} providerName="Codex" />);
      });
      await act(async () => {
        root.render(<ChatSetupState pane={ready} view={chatSetupView(ready)} providerName="Codex" />);
      });
      assert(!host.querySelector('[data-testid="chat-setup-state"]'), "reduced motion skips exit overlap");
    } finally {
      if (matchMediaDescriptor) Object.defineProperty(window, "matchMedia", matchMediaDescriptor);
      else Reflect.deleteProperty(window, "matchMedia");
    }
  });

  await test("ready renders nothing (empty-state text stays the caller's job)", async () => {
    const p = pane("agent_ready");
    const host = await render(
      <ChatSetupState pane={p} view={chatSetupView(p)} providerName="Claude Code" />,
    );
    assert(!host.querySelector('[data-testid="chat-setup-state"]'), "no setup card");
    assert(!host.querySelector('[data-testid="chat-launch-failed"]'), "no failed card");
  });

  await test("an attached chat is authoritative over a late failed boot stage", async () => {
    const p = pane("spawn_failed", "stale launch result");
    const host = await render(
      <ChatSetupState pane={p} view={chatSetupView(p, true)} providerName="Codex" />,
    );
    assert(!host.querySelector('[data-testid="chat-launch-failed"]'), "attached chat is never covered by a stale failure");
    assert(!host.querySelector('[data-testid="chat-setup-state"]'), "attached chat has no launch overlay");
  });

  console.log(failures === 0 ? "ChatSetupState tests: all passed" : `ChatSetupState tests: ${failures} failed`);
  if (failures > 0) process.exit(1);
}

void main();
