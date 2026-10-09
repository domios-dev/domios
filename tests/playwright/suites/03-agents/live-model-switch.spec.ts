/**
 * WHAT: Status-line model and effort switching on real DEV provider tabs.
 * WHY: Optimistic capsules cannot prove which model handled a turn.
 * ORACLE: Exact own-session turn records, unique Chat bubbles, identity and measured transitions.
 */
import { performance } from "node:perf_hooks";
import { writeFileSync } from "node:fs";
import { expect, test } from "../../helpers/fixtures";
import { dc, identity, records, turnEvidence, assertModel, restoreManifest, grokEffortEvidence, switchModels, deliveryLog, type Descriptor, type Model, type RestoreTab } from "../../helpers/live-model-switch";
import { openAgent, readChat, type AgentUi } from "../16-chat-view/chat-send-matrix.live.helpers";
import { detectChatPrefix } from "../../../../src/lib/chatPrefixes";
import { completedTurn, waitForCompletedTurn } from "../../helpers/live-switch-completion";

const providers = (process.env.PW_SWITCH_PROVIDERS ?? "claude,codex,grok").split(",");
if (providers.some(id => !["claude", "codex", "opencode", "grok"].includes(id))) throw new Error("Unknown switch matrix provider");
const restored = restoreManifest();
const selectedSteps = process.env.PW_SWITCH_STEPS?.split(",");
if (selectedSteps?.some(step => !["idle", "effort", "combined", "busy", "back"].includes(step))) throw new Error("Unknown switch scenario");
const repeats = restored ? 1 : Number(process.env.PW_SWITCH_RUNS ?? 3);
if (!Number.isInteger(repeats) || repeats < 1 || repeats > 3) throw new Error("PW_SWITCH_RUNS must be 1..3");
const startRun = Number(process.env.PW_SWITCH_RUN_NUMBER ?? 1);
if (!Number.isInteger(startRun) || startRun < 1 || startRun + repeats > 4) throw new Error("Invalid matrix run range");
if (process.env.PW_SWITCH_WORKSPACE && repeats !== 1) throw new Error("A prepared scratch workspace may serve exactly one matrix");
type SwitchRow = { run: number; sha: string; provider: string; step: string; verdict: string; detail?: string;
  tabId?: string; model?: string; effort?: string; effortEvidence?: string; mode?: string; closeMs?: number; pendingMs?: number; confirmMs?: number; failureMs?: number; recordModel?: string;
  applyingMs?: number; applyingLowerBoundMs?: number };
type SwitchProbe = { applying: boolean; closed: boolean; errorToast: boolean; applyingStarted?: number; applyingEnded?: number };
type ProbeWindow = typeof window & { __switchProbe: SwitchProbe; __switchObserver: { disconnect(): void }; __switchObserverOwner: string };

test.describe.configure({ mode: "default" });
for (let run = startRun; run < startRun + repeats; run++) {
  test(`@live @model-switch matrix run ${run}${restored ? " restored" : ""}`, async ({ api, ui }, info) => {
    test.setTimeout(3_600_000);
    expect(process.env.PW_TARGET, "attach-only DEV required; no lifecycle/reset").toBe("dev");
    const sha = (await api.get<{ build_sha: string }>("/health")).build_sha;
    expect(sha).toBe(process.env.PW_SWITCH_DEV_SHA);
    const descriptors = await dc<Descriptor[]>("agent", "providers");
    const originalWorkspace = (await api.workspaces()).find(ws => ws.is_active)?.id;
    const workspaceName = `SwitchQA-${Date.now()}-${run}`;
    let workspace = restored?.workspace ?? process.env.PW_SWITCH_WORKSPACE;
    const rows: SwitchRow[] = [];
    const failures: string[] = [];
    const retained: RestoreTab[] = [];
    if (!workspace) {
      await dc("workspace", "create", workspaceName, "--provision");
      workspace = (await api.workspaces()).find(ws => ws.name === workspaceName)?.id;
    }
    expect(workspace, "dedicated scratch workspace created").toBeTruthy();
    if (!workspace) throw new Error("scratch workspace missing");
    const workspaceId = workspace;
    if (restored || process.env.PW_SWITCH_WORKSPACE) {
      const scratch = (await api.workspaces()).find(ws => ws.id === workspaceId);
      expect(scratch?.name, "restore may only touch a dedicated scratch workspace").toMatch(/^SwitchQA-/);
      if (restored) for (const prior of restored.tabs) expect(scratch?.tab_ids).toContain(prior.tabId);
      else expect(scratch?.tab_ids, "prepared scratch workspace must be empty before the matrix").toEqual([]);
    }
    try {
      for (const providerId of providers) {
        let agent: AgentUi | undefined;
        let step = "setup";
        const descriptor = descriptors.find(p => p.id === providerId)!;
        try {
          expect(descriptor, `provider ${providerId} installed`).toBeDefined();
          expect(descriptor.model_switch, "restart modes must be explicitly reviewed, not silently exercised").not.toBe("restart_resume");
          const choices = descriptor.available_models;
          const { a, b } = switchModels(descriptor);
          expect(a, "model A exists").toBeDefined();
          expect(b, "model B exists").toBeDefined();
          const prior = restored?.tabs.find(tab => tab.provider === providerId);
          if (restored && !prior) throw new Error(`No retained scratch ${providerId} tab; restore cannot spawn a replacement`);
          agent = await openAgent(api, ui, async () => {
            if (prior) return { tab_id: prior.tabId };
            const spawned = await dc<{ tab_id?: string; id?: string }>("agent", "spawn", providerId, "--workspace", workspaceId,
              "--title", `Switch${providerId}`, "--model", a.id, "--non-user-facing");
            const tabId = spawned.tab_id ?? spawned.id;
            if (!tabId) throw new Error("spawn returned no scratch tab id");
            return { tab_id: tabId };
          }, descriptor, workspaceId, `Switch${providerId}`);
          const target = agent;
          const capsule = ui.page.locator(`[data-tab-id="${target.tabId}"]`).getByTestId("status-model-trigger").filter({ visible: true });
          const picker = ui.page.getByTestId("status-model-picker");
          await expect(capsule).toHaveCount(1);
          let baseline = await identity(api, target.tabId);
          expect(baseline.session_binding_nonce).toBeTruthy();
          if (prior) {
            expect(baseline.agent_session_id, "DEV restore preserves exact session").toBe(prior.session);
            expect(baseline.session_binding_nonce, "DEV restart replaces the launch nonce").not.toBe(prior.nonce);
            expect(baseline.pty_id, "DEV restart replaces the PTY").not.toBe(prior.pty);
          }
          let turnIndex = 0;
          const send = async (text: string) => {
            await target.composer.fill(text);
            await target.composer.press("Enter");
          };
          const ack = async (model: Model, effort?: string, switchedAtMs?: number) => {
            console.log(`[switch-step] ${providerId}/${step} ACK ${model.id}`);
            const effortProof = providerId === "grok" && effort && switchedAtMs !== undefined
              ? await grokEffortEvidence(baseline.agent_session_id, switchedAtMs, effort) : undefined;
            if (effortProof) {
              writeFileSync(info.outputPath(`${providerId}-${step}-session-effort.json`), JSON.stringify(effortProof, null, 2));
              await info.attach(`${providerId}-${step}-session-effort.json`, { path: info.outputPath(`${providerId}-${step}-session-effort.json`), contentType: "application/json" });
            }
            const marker = `SWITCH_ACK_${run}_${providerId}_${Date.now()}_${turnIndex++}`;
            const before = await readChat(api, target.tabId);
            const text = `Reply with only this token, no punctuation or tools: ${marker}`;
            await send(text);
            await expect(target.chat.getByRole("article", { name: "assistant message" }).filter({ hasText: marker })).toHaveCount(1, { timeout: 180_000 });
            await expect(target.chat.locator('[data-agent-state="idle"]')).toHaveCount(1, { timeout: 180_000 });
            const after = await readChat(api, target.tabId);
            await info.attach(`${providerId}-${step}-chat.json`, { body: JSON.stringify({ before, after }, null, 2), contentType: "application/json" });
            const current = await identity(api, target.tabId);
            expect(current.pty_id).toBe(baseline.pty_id);
            expect(current.session_binding_nonce).toBe(baseline.session_binding_nonce);
            if (!baseline.agent_session_id && step === "baseline") baseline = current;
            const evidence = await records(providerId, current.agent_session_id);
            writeFileSync(info.outputPath(`${providerId}-${step}-raw.json`), JSON.stringify(evidence, null, 2));
            const exact = turnEvidence(providerId, evidence, marker);
            await info.attach(`${providerId}-${step}-records.json`, { body: JSON.stringify({ path: evidence.path, ...exact }, null, 2), contentType: "application/json" });
            assertModel(providerId, exact.model, model.id);
            if (effort && !effortProof) expect(exact.effort, "effort must come from the exact provider turn record").toBe(effort);
            const realUser = (m: { type: string; content: string }) => {
              const prefix = detectChatPrefix(m.content);
              return m.type === "user" && !(prefix?.origin.kind === "command" && prefix.body === "");
            };
            expect(after.messages.filter(realUser).length).toBe(before.messages.filter(realUser).length + 1);
            const newMessages = after.messages.filter(m => !before.messages.some(old => old.id === m.id));
            expect(newMessages.filter(realUser).map(m => m.content)).toEqual([text]);
            expect(newMessages.filter(m => m.type === "assistant").map(m => m.content.trim())).toEqual([marker]);
            await expect(target.chat.getByRole("article", { name: "user message" }).filter({ hasText: marker })).toHaveCount(1);
            await expect(target.chat.getByRole("article", { name: "user message" }).filter({ hasText: /<command-name>|<local-command-stdout>|^\/(?:model|effort)\b/ })).toHaveCount(0);
            expect(await ui.page.evaluate(() => (window as ProbeWindow).__switchProbe?.errorToast ?? false), "no own-tab model-switch failure toast during Apply or ACK").toBe(false);
            expect(current.agent_session_id).toBe(baseline.agent_session_id);
            expect(current.pty_id).toBe(baseline.pty_id);
            expect(current.session_binding_nonce).toBe(baseline.session_binding_nonce);
            console.log(`[switch-step] ${providerId}/${step} ACK verified`);
            return exact.model;
          };
          const apply = async (model: Model, effort?: string, busy = false) => {
            console.log(`[switch-step] ${providerId}/${step} picker ${model.id} ${effort ?? ""}`);
            const row: SwitchRow = { run, sha, provider: providerId, tabId: target.tabId, step, model: model.id, effort, mode: descriptor.model_switch, verdict: "pending" };
            rows.push(row);
            await capsule.click();
            await expect(picker).toBeVisible();
            const showAll = picker.getByRole("button", { name: "Show all models", exact: true });
            if (await showAll.isVisible()) await showAll.click();
            await picker.locator(`[role="option"][title="${model.id}"]`).click();
            if (effort) await picker.locator(`[aria-label="Reasoning"] [role="tab"][data-value="${effort}"]`).click();
            await capsule.evaluate((trigger, tabId) => {
              (window as ProbeWindow).__switchObserver?.disconnect();
              const host = trigger.parentElement!;
              const titlebar = document.querySelector('[data-testid="titlebar"]');
              const state: SwitchProbe = { applying: false, closed: false, errorToast: false };
              (window as ProbeWindow).__switchProbe = state;
              const observe = () => {
                if (trigger.getAttribute("aria-expanded") !== "false") return;
                state.closed = true;
                const applying = Array.from(host.children).some(node => node.getAttribute("role") === "status" && node.textContent === "Applying…");
                if (applying) { state.applying = true; state.applyingStarted ??= window.performance.now(); }
                else if (state.applyingStarted !== undefined) state.applyingEnded ??= window.performance.now();
              };
              const observer = new MutationObserver(observe);
              observer.observe(host, { childList: true, subtree: true, characterData: true, attributes: true, attributeFilter: ["aria-expanded"] });
              const notificationId = `model-switch:${tabId}`;
              const toastObserver = new MutationObserver(changes => {
                for (const change of changes) for (const node of change.addedNodes) {
                  if (node instanceof Element && (node.getAttribute("data-notification-id") === notificationId ||
                    node.querySelector(`[data-notification-id="${notificationId}"]`))) state.errorToast = true;
                }
              });
              if (titlebar) {
                state.errorToast = titlebar.querySelector(`[data-notification-id="${notificationId}"]`) !== null;
                toastObserver.observe(titlebar, { childList: true, subtree: true });
              }
              (window as ProbeWindow).__switchObserverOwner = tabId;
              (window as ProbeWindow).__switchObserver = { disconnect: () => { observer.disconnect(); toastObserver.disconnect(); } };
            }, target.tabId);
            const started = performance.now();
            const switchedAtMs = Date.now();
            const captureApplyingDuration = async () => {
              const timing = await ui.page.evaluate(() => {
                const probe = (window as ProbeWindow).__switchProbe;
                if (probe.applyingStarted === undefined) return {};
                return probe.applyingEnded === undefined
                  ? { applyingLowerBoundMs: window.performance.now() - probe.applyingStarted }
                  : { applyingMs: probe.applyingEnded - probe.applyingStarted };
              });
              Object.assign(row, timing);
            };
            try {
            await picker.getByRole("button", { name: "Apply", exact: true }).click();
            await expect(picker).toHaveCount(0, { timeout: 1_000 });
            const closeMs = performance.now() - started;
            row.closeMs = closeMs;
            const probe = await ui.page.evaluate(() => {
              return (window as ProbeWindow).__switchProbe;
            });
            expect(probe.closed).toBe(true);
            expect(probe.applying, "Apply visibly reports Applying… before settling").toBe(true);
            if (busy || descriptor.model_switch === "next_turn") {
              await expect(capsule).toHaveAttribute("aria-label", /after this turn/, { timeout: 1_000 });
            } else {
              await expect(ui.page.getByRole("status").filter({ hasText: /^Applying…$/ })).toHaveCount(0, { timeout: 3_000 });
              await expect(capsule).not.toHaveAttribute("aria-label", /after this turn/, { timeout: 3_000 });
            }
            await expect(capsule).toContainText(model.display_name);
            if (effort) await expect(capsule).toContainText(effort);
            const pendingMs = performance.now() - started;
            row.pendingMs = pendingMs;
            if (!busy && descriptor.model_switch === "live_command") expect(pendingMs).toBeLessThan(3_000);
            await captureApplyingDuration();
            return { started, switchedAtMs, row };
            } catch (error) {
              row.failureMs = performance.now() - started;
              row.verdict = "fail";
              await captureApplyingDuration().catch(() => {});
              throw error;
            }
          };
          step = "baseline";
          await ack(a);
          const scenario = async (name: string, action: () => Promise<void>) => {
            if (selectedSteps && !selectedSteps.includes(name)) return;
            step = name;
            try { await action(); } catch (error) {
              const detail = error instanceof Error ? error.stack : String(error);
              console.log(`[switch-failure] ${providerId}/${step}: ${detail}`);
              failures.push(`${providerId}/${step}: ${detail}`);
              const row = [...rows].reverse().find(row => row.provider === providerId && row.step === step);
              if (row) Object.assign(row, { verdict: "fail", detail });
              else rows.push({ run, sha, provider: providerId, step, verdict: "fail", detail });
              const screenshot = info.outputPath(`${providerId}-${step}.png`);
              await ui.page.screenshot({ path: screenshot, timeout: 15_000 });
              await info.attach(`${providerId}-${step}.png`, { path: screenshot, contentType: "image/png" });
              const failureState = { detail, identity: await identity(api, target.tabId), chat: await readChat(api, target.tabId) };
              writeFileSync(info.outputPath(`${providerId}-${step}-failure.json`), JSON.stringify(failureState, null, 2));
              const diagnostic = await Promise.allSettled([
                dc("tab", "get-text", "--tab", target.tabId),
                dc("tab", "inspect", "--tab", target.tabId, "--lifecycle"),
                dc("tab", "chat", "--tab", target.tabId),
                deliveryLog(target.tabId),
              ]);
              writeFileSync(info.outputPath(`${providerId}-${step}-pty.json`), JSON.stringify(diagnostic, null, 2));
              await ui.page.keyboard.press("Escape");
            }
          };
          await scenario("idle", async () => {
            const change = await apply(b!);
            change.row.recordModel = await ack(b!);
            change.row.confirmMs = performance.now() - change.started;
            change.row.verdict = "pass";
          });
          const efforts = (b!.reasoning_efforts ?? descriptor.reasoning_efforts).filter(e => e.id !== "default");
          if (efforts.length >= 2) {
            const currentEffort = (await identity(api, target.tabId)).reasoning_effort_identity?.actual;
            const effortB = efforts.find(e => e.id !== currentEffort)!.id;
            await scenario("effort", async () => {
            const change = await apply(b!, effortB);
            change.row.recordModel = await ack(b!, effortB, change.switchedAtMs);
            change.row.effortEvidence = providerId === "grok" ? "session-level evidence" : providerId === "opencode" ? "turn-config evidence (user.variant)" : "turn record";
            change.row.confirmMs = performance.now() - change.started;
            change.row.verdict = "pass";
            });
            await scenario("combined", async () => {
            const common = (a.reasoning_efforts ?? descriptor.reasoning_efforts).find(e => e.id !== effortB && e.id !== "default");
            expect(common, "second effort supported by model A").toBeDefined();
            const change = await apply(a, common!.id);
            change.row.recordModel = await ack(a, common!.id, change.switchedAtMs);
            change.row.effortEvidence = providerId === "grok" ? "session-level evidence" : providerId === "opencode" ? "turn-config evidence (user.variant)" : "turn record";
            change.row.confirmMs = performance.now() - change.started;
            change.row.verdict = "pass";
            });
          } else rows.push({ run, sha, provider: providerId, step: "effort+combined", verdict: "unsupported capability" });
          await scenario("busy", async () => {
          await expect(target.chat.locator('[data-agent-state="idle"]')).toHaveCount(1, { timeout: 60_000 });
          await expect(capsule).not.toHaveAttribute("aria-label", /after this turn/, { timeout: 15_000 });
          const floor = target.events.latestSequence();
          const marker = `SWITCH_BUSY_${run}_${providerId}_${Date.now()}`;
          await send(`Write 600 numbered sentences about trees, one per line, starting with ${marker}. Do not use tools.`);
          await expect(target.chat.getByRole("article", { name: "user message" }).filter({ hasText: marker })).toHaveCount(1);
          await target.events.waitFor(e => e.seq > floor && e.event_type === "tab-status-changed" && e.data.tab_id === target.tabId && e.data.status === "Working", 60_000, "long turn starts");
          const startedRecords = await records(providerId, baseline.agent_session_id);
          writeFileSync(info.outputPath(`${providerId}-${step}-start-raw.json`), JSON.stringify(startedRecords, null, 2));
          const markedUsers = startedRecords.records.filter(row => JSON.stringify(row).includes(marker) &&
            (row.type === "user" || row.role === "user" || row.payload?.role === "user" || row.params?.update?.sessionUpdate === "user_message_chunk"));
          expect(markedUsers, "Working belongs to the exact persisted long user turn").toHaveLength(1);
          await expect(target.chat.locator('[data-agent-state="working"]')).toHaveCount(1);
          const busyBefore = await identity(api, target.tabId);
          const activeModels = choices.filter(m => m.id === busyBefore.agent_session_model || m.id.endsWith(`/${busyBefore.agent_session_model}`));
          expect(activeModels, "session model resolves to one catalogue model").toHaveLength(1);
          const busyModel = [a, b].find(m => m.id !== activeModels[0].id)!;
          const change = await apply(busyModel, undefined, true);
          await expect(target.chat.locator('[data-agent-state="working"]'), "Apply happened before long turn completed").toHaveCount(1);
          expect((await identity(api, target.tabId)).agent_session_model).toBe(busyBefore.agent_session_model);
          const responsiveness: Array<{ at: string; elapsedMs: number; completedAtSnapshot: boolean; error?: string }> = [];
          const cdp = providerId === "opencode" ? await ui.page.context().newCDPSession(ui.page) : undefined;
          const probeRenderer = cdp ? async (snapshot: Awaited<ReturnType<typeof records>>) => {
            let completedAtSnapshot = false;
            try { completedTurn(providerId, snapshot, marker); completedAtSnapshot = true; } catch {}
            const started = performance.now();
            const at = new Date().toISOString();
            const signal = AbortSignal.timeout(15_000);
            let expired!: () => void;
            const deadline = new Promise<never>((_resolve, reject) => {
              expired = () => reject(new Error("Renderer CDP ping exceeded 15s"));
              signal.addEventListener("abort", expired, { once: true });
            });
            try {
              const result = await Promise.race([cdp.send("Runtime.evaluate", { expression: "1", returnByValue: true }), deadline]);
              expect(result.result.value).toBe(1);
              responsiveness.push({ at, elapsedMs: performance.now() - started, completedAtSnapshot });
            } catch (error) { responsiveness.push({ at, elapsedMs: performance.now() - started, completedAtSnapshot, error: String(error) }); }
            finally { signal.removeEventListener("abort", expired); }
          } : undefined;
          let rawBusy;
          try { rawBusy = await waitForCompletedTurn(providerId, startedRecords, marker, 240_000, probeRenderer); }
          finally {
            if (cdp) {
              writeFileSync(info.outputPath(`${providerId}-${step}-cdp-responsiveness.json`), JSON.stringify(responsiveness, null, 2));
              await cdp.detach().catch(() => {});
            }
          }
          writeFileSync(info.outputPath(`${providerId}-${step}-raw.json`), JSON.stringify(rawBusy, null, 2));
          expect(responsiveness.filter(sample => sample.error), "renderer responds to CDP during OpenCode stream").toEqual([]);
          if (cdp) expect(responsiveness.some(sample => !sample.completedAtSnapshot), "CDP measurement covers the active long turn").toBe(true);
          const busyRecords = completedTurn(providerId, rawBusy, marker);
          assertModel(providerId, busyRecords.model, activeModels[0].id);
          await expect(target.chat.locator('[data-agent-state="idle"]')).toHaveCount(1, { timeout: 15_000 });
          if (descriptor.model_switch === "live_command") await expect(capsule).not.toHaveAttribute("aria-label", /after this turn/, { timeout: 3_000 });
          change.row.recordModel = await ack(busyModel);
          change.row.confirmMs = performance.now() - change.started;
          change.row.verdict = "pass";
          });
          await scenario("back", async () => {
          const change = await apply(a);
          change.row.recordModel = await ack(a);
          change.row.confirmMs = performance.now() - change.started;
          change.row.verdict = "pass";
          await expect(ui.page.getByText("Model switch did not apply", { exact: true })).toHaveCount(0);
          });
          retained.push({ provider: providerId, tabId: target.tabId, session: baseline.agent_session_id, nonce: baseline.session_binding_nonce, pty: baseline.pty_id, a: a.id, b: b!.id });
        } catch (error) {
          const detail = error instanceof Error ? error.stack : String(error);
          console.log(`[switch-failure] ${providerId}/${step}: ${detail}`);
          failures.push(`${providerId}/${step}: ${detail}`);
          rows.push({ run, sha, provider: providerId, step, verdict: "fail", detail });
          await info.attach(`${providerId}-${step}.png`, { body: await ui.page.screenshot({ timeout: 15_000 }), contentType: "image/png" });
          if (agent) await info.attach(`${providerId}-${step}-identity.json`, { body: JSON.stringify(await identity(api, agent.tabId)), contentType: "application/json" });
          if (agent) {
            const current = await identity(api, agent.tabId);
            await info.attach(`${providerId}-${step}-failure-chat.json`, { body: JSON.stringify(await readChat(api, agent.tabId), null, 2), contentType: "application/json" });
            if (current.agent_session_id) {
              await info.attach(`${providerId}-${step}-failure-raw.json`, { body: JSON.stringify(await records(providerId, current.agent_session_id), null, 2), contentType: "application/json" });
            }
          }
          await ui.page.keyboard.press("Escape");
        } finally {
          if (agent) {
            const signal = AbortSignal.timeout(15_000);
            let expired!: () => void;
            const deadline = new Promise<never>((_resolve, reject) => {
              expired = () => reject(new Error("Own probe disconnect deadline"));
              signal.addEventListener("abort", expired, { once: true });
            });
            try {
              await Promise.race([ui.page.evaluate(tabId => {
                if ((window as ProbeWindow).__switchObserverOwner === tabId) (window as ProbeWindow).__switchObserver?.disconnect();
              }, agent.tabId), deadline]);
            } catch (error) { console.log(`[switch-probe-cleanup] ${agent.tabId}: ${String(error)}`); }
            finally { signal.removeEventListener("abort", expired); }
          }
          if (agent) await info.attach(`${providerId}-stream-resyncs.json`, { body: JSON.stringify(agent.events.resyncs, null, 2), contentType: "application/json" });
          await agent?.events.close();
        }
      }
    } finally {
      const summary = { run, sha, workspace, requestedSteps: selectedSteps ?? ["idle", "effort", "combined", "busy", "back"], rows, failures, tabs: retained };
      writeFileSync(info.outputPath("matrix.json"), JSON.stringify(summary, null, 2));
      await info.attach("matrix.json", { path: info.outputPath("matrix.json"), contentType: "application/json" });
      console.log(JSON.stringify(summary));
      if (run === startRun + repeats - 1 && !restored && process.env.PW_SWITCH_KEEP === "1") {
        writeFileSync(info.outputPath("restore-manifest.json"), JSON.stringify(summary, null, 2));
      } else await dc("workspace", "close", workspaceId, "--force");
      if (originalWorkspace && originalWorkspace !== workspace) await api.focusWorkspace(originalWorkspace);
    }
    expect(failures, "all requested providers and steps must pass; no retries").toEqual([]);
  });
}
