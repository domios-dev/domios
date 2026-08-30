import { Window } from "happy-dom";
import type { ReactElement } from "react";
import type { Root } from "react-dom/client";
import type { CronJob } from "../../lib/types";
import type { Service } from "./types";
import type { DrawerState } from "./ServiceDrawer";
import type { MeshResourceActionAdapter } from "./meshResourceActions";

let useReactState!: typeof import("react").useState;
let MeshServiceDrawer!: typeof import("./ServiceDrawer").ServiceDrawer;

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/" });
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
  Object.defineProperty(globalThis, "Element", { value: window.Element, configurable: true });
  Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
  Object.defineProperty(globalThis, "Event", { value: window.Event, configurable: true });
  for (const name of ["HTMLElement", "HTMLInputElement", "HTMLTextAreaElement", "HTMLSelectElement"] as const) {
    Object.defineProperty(globalThis, name, { value: window[name], configurable: true });
  }
  Object.defineProperty(window, "confirm", { value: () => true, writable: true, configurable: true });
}

function resource(overrides: Partial<Service> & Pick<Service, "id" | "name" | "kind">): Service {
  return {
    health_mode: "http",
    health_interval: 60,
    health: { status: "healthy" },
    push_enabled: false,
    registered_at: 1,
    updated_at: 1,
    enabled: true,
    machine_id: "local-machine",
    machine_name: "Local machine",
    capabilities: {
      get: { allowed: true },
      history: { allowed: true },
      delete: { allowed: true },
      update_schedule: { allowed: true },
      trigger: { allowed: true },
      enable: { allowed: true },
      disable: { allowed: true },
      cancel: { allowed: true },
    },
    ...overrides,
    id: overrides.id,
    name: overrides.name,
    kind: overrides.kind,
  };
}

function cronJob(): CronJob {
  return {
    id: "cron-backup",
    name: "Backup",
    description: null,
    workspace_id: "general",
    schedule: "0 3 * * *",
    enabled: true,
    agent: "test",
    model: null,
    task: "backup",
    cwd: null,
    role: null,
    oneshot: false,
    max_history: 10,
    created_at: 1,
    last_run: null,
    next_run: null,
    jitter_max_ms: null,
  };
}

function adapter(overrides: Partial<MeshResourceActionAdapter> = {}): MeshResourceActionAdapter {
  return {
    get: async () => cronJob(),
    history: async () => [],
    trigger: async () => ({ status: "Running" }),
    setEnabled: async () => ({ enabled: true }),
    cancel: async () => ({ status: "Cancelled" }),
    updateSchedule: async () => ({ schedule: "0 3 * * *" }),
    remove: async () => undefined,
    regenerateToken: async () => "token",
    ...overrides,
  };
}

async function main() {
  setupDom();
  const [{ act, useState }, { createRoot }, { ServiceDrawer }] = await Promise.all([
    import("react"),
    import("react-dom/client"),
    import("./ServiceDrawer"),
  ]);
  useReactState = useState;
  MeshServiceDrawer = ServiceDrawer;
  const origin = document.createElement("button");
  origin.textContent = "origin";
  document.body.append(origin);

  async function render(ui: ReactElement): Promise<{ host: HTMLDivElement; root: Root }> {
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    await act(async () => root.render(ui));
    return { host, root };
  }

  const daemon = resource({ id: "daemon-1", name: "Indexer", kind: "daemon", capabilities: { get: { allowed: true }, delete: { allowed: true } } });
  let removeResolve: (() => void) | undefined;
  const remove = () => new Promise<void>((resolve) => { removeResolve = resolve; });
  const removeAdapter = adapter({ remove });
  const removed: string[] = [];
  await act(async () => origin.focus());
  const { host: daemonHost, root: daemonRoot } = await render(
    <DrawerHarness service={daemon} actionAdapter={removeAdapter} onClose={() => removed.push("closed")} />,
  );
  const removeButton = Array.from(daemonHost.querySelectorAll<HTMLButtonElement>("button")).find((button) => button.textContent?.includes("Remove registry entry"));
  assert(removeButton?.textContent?.includes("Remove registry entry"), "daemon exposes registry removal");
  assert(!daemonHost.textContent?.toLowerCase().includes("stop"), "daemon detail never promises process stop");
  let confirmation = "";
  window.confirm = (message = "") => { confirmation = message; return true; };
  await act(async () => { removeButton?.click(); await Promise.resolve(); });
  assert(confirmation.includes("Indexer") && confirmation.includes("Local machine"), "confirmation identifies resource and target machine");
  assert(removeButton?.disabled === true, "destructive action is busy while target outcome is pending");
  assert(!removed.length, "drawer stays open until target outcome returns");
  removeResolve?.();
  await act(async () => { await Promise.resolve(); await Promise.resolve(); });
  assert(removed[0] === "closed", "successful deletion closes the drawer");
  assert(document.activeElement === origin, "successful deletion returns focus to the originating row");
  await act(async () => daemonRoot.unmount());

  let rejected = false;
  const failedService = resource({ id: "daemon-2", name: "Failing", kind: "daemon", capabilities: { get: { allowed: true }, delete: { allowed: true } } });
  const failedAdapter = adapter({ remove: async () => { throw new Error("target rejected"); } });
  const { host: failedHost, root: failedRoot } = await render(<MeshServiceDrawer state={{ kind: "service", id: failedService.id }} services={[failedService]} peers={[]} thisMachineName="Local machine" onClose={() => {}} onActionDone={() => {}} actionAdapter={failedAdapter} onAnnouncement={() => { rejected = true; }} />);
  const failedButton = Array.from(failedHost.querySelectorAll<HTMLButtonElement>("button")).find((button) => button.textContent?.includes("Remove registry entry"));
  await act(async () => { failedButton?.focus(); failedButton?.click(); await Promise.resolve(); });
  assert(rejected, "target failure is announced through the live-region handoff");
  assert(failedHost.querySelector("[data-testid='mesh-resource-announcement']")?.textContent?.includes("rejected or failed") === true, "target failure remains observable");
  assert(failedHost.querySelector("[data-testid='mesh-service-drawer']")?.contains(document.activeElement) === true, "focus remains in drawer after failure");
  await act(async () => failedRoot.unmount());

  const remoteSchedule = resource({
    id: "cron_backup",
    name: "Backup",
    kind: "cron",
    machine_id: "remote-machine",
    machine_name: "Remote machine",
    source_peer_id: "peer-1",
    source_peer: "Peer label",
    schedule: "0 3 * * *",
    last_run_status: "running",
  });
  const calls: string[] = [];
  const remoteAdapter = adapter({
    history: async () => { calls.push("history"); return []; },
    get: async () => { calls.push("get"); return cronJob(); },
    updateSchedule: async () => { calls.push("remote-update"); return { ok: true }; },
    cancel: async () => { calls.push("cancel"); return { ok: true }; },
  });
  const { host: remoteHost, root: remoteRoot } = await render(<MeshServiceDrawer state={{ kind: "service", id: remoteSchedule.id, sourcePeerId: remoteSchedule.source_peer_id }} services={[remoteSchedule]} peers={[]} thisMachineName="Local machine" onClose={() => {}} onActionDone={() => {}} actionAdapter={remoteAdapter} />);
  assert(remoteHost.textContent?.includes("Remote machine") && remoteHost.textContent?.includes("edit schedule"), "remote schedule summary names target and capabilities");
  const cancelButton = Array.from(remoteHost.querySelectorAll<HTMLButtonElement>("button")).find((button) => button.textContent?.includes("Cancel running"));
  assert(cancelButton != null, "running schedules expose cancel when capability allows");
  const editButton = Array.from(remoteHost.querySelectorAll<HTMLButtonElement>("button")).find((button) => button.textContent === "Edit");
  await act(async () => editButton?.click());
  const scheduleInput = remoteHost.querySelector<HTMLInputElement>("input[aria-label='Schedule expression']");
  await act(async () => { if (scheduleInput) { scheduleInput.value = "15 4 * * *"; scheduleInput.dispatchEvent(new window.Event("input", { bubbles: true })); } });
  window.confirm = (message = "") => { confirmation = message; return true; };
  const saveButton = Array.from(remoteHost.querySelectorAll<HTMLButtonElement>("button")).find((button) => button.textContent === "Save");
  await act(async () => saveButton?.click());
  assert(calls.includes("remote-update") && confirmation.includes("Remote machine"), "remote schedule edit uses target adapter and target confirmation");
  await act(async () => cancelButton?.click());
  assert(calls.includes("cancel"), "cancel uses the injected target outcome adapter");
  assert(confirmation.includes("Backup") && confirmation.includes("Remote machine"), "remote cancel confirmation identifies schedule and target machine");
  await act(async () => remoteRoot.unmount());

  const remoteOrigin = document.createElement("button");
  remoteOrigin.textContent = "remote origin";
  document.body.append(remoteOrigin);
  let remoteDeleteResolve: (() => void) | undefined;
  let remoteDeleted = false;
  const remoteDelete = () => new Promise<void>((resolve) => { remoteDeleteResolve = resolve; });
  await act(async () => remoteOrigin.focus());
  const { host: remoteDeleteHost, root: remoteDeleteRoot } = await render(<DrawerHarness service={remoteSchedule} actionAdapter={adapter({ remove: remoteDelete })} onClose={() => { remoteDeleted = true; }} />);
  let remoteDeleteConfirmation = "";
  window.confirm = (message = "") => { remoteDeleteConfirmation = message; return true; };
  const remoteDeleteButton = Array.from(remoteDeleteHost.querySelectorAll<HTMLButtonElement>("button")).find((button) => button.textContent?.includes("Remove schedule"));
  await act(async () => { remoteDeleteButton?.click(); await Promise.resolve(); });
  assert(remoteDeleteConfirmation.includes("Backup") && remoteDeleteConfirmation.includes("Remote machine"), "remote deletion confirmation identifies schedule and target machine");
  assert(remoteDeleteButton?.disabled === true && !remoteDeleted, "remote deletion waits for the target outcome before closing");
  assert(remoteDeleteHost.querySelector("[data-testid='mesh-service-drawer']")?.contains(document.activeElement) === true, "focus remains in drawer while remote deletion is pending");
  remoteDeleteResolve?.();
  await act(async () => { await Promise.resolve(); await Promise.resolve(); });
  assert(remoteDeleted && document.activeElement === remoteOrigin, "remote deletion closes and returns focus after target success");
  await act(async () => remoteDeleteRoot.unmount());

  const localCancelCalls: string[] = [];
  let localCancelConfirmation = "";
  const localRunning = resource({ id: "cron_local_running", name: "Local run", kind: "cron", last_run_status: "running" });
  const { host: localCancelHost, root: localCancelRoot } = await render(<MeshServiceDrawer state={{ kind: "service", id: localRunning.id }} services={[localRunning]} peers={[]} thisMachineName="Local machine" onClose={() => {}} onActionDone={() => {}} actionAdapter={adapter({ cancel: async () => { localCancelCalls.push("cancel"); return { ok: true }; } })} />);
  window.confirm = (message = "") => { localCancelConfirmation = message; return true; };
  const localCancelButton = Array.from(localCancelHost.querySelectorAll<HTMLButtonElement>("button")).find((button) => button.textContent?.includes("Cancel running"));
  await act(async () => localCancelButton?.click());
  assert(localCancelCalls.length === 1, "local cancel invokes only after the returned action is requested");
  assert(localCancelConfirmation.includes("Local run") && localCancelConfirmation.includes("Local machine"), "local cancel confirmation identifies schedule and target machine");
  await act(async () => localCancelRoot.unmount());

  const stale = resource({ id: "cron_stale", name: "Stale", kind: "cron", capabilities: { history: { allowed: true }, get: { allowed: true }, trigger: { allowed: false, unavailable_reason: "stale" }, disable: { allowed: false, unavailable_reason: "stale" }, delete: { allowed: false, unavailable_reason: "stale" } }, stale: true, provenance: { freshness: "stale", offline: false } });
  const staleCalls: string[] = [];
  const { host: staleHost, root: staleRoot } = await render(<MeshServiceDrawer state={{ kind: "service", id: stale.id }} services={[stale]} peers={[]} thisMachineName="Local machine" onClose={() => {}} onActionDone={() => {}} actionAdapter={adapter({ trigger: async () => { staleCalls.push("trigger"); }, setEnabled: async () => { staleCalls.push("toggle"); } })} />);
  assert(staleHost.querySelector("[data-testid='mesh-operation-unavailable-trigger']")?.textContent?.includes("stale") === true, "stale capability reason is shown exactly");
  assert(Array.from(staleHost.querySelectorAll("button")).find((button) => button.textContent?.includes("Trigger now"))?.disabled === true, "stale trigger is disabled");
  assert(staleCalls.length === 0, "stale capability never invokes a mutation adapter");
  await act(async () => staleRoot.unmount());

  for (const [label, freshness, reason] of [["offline", "offline", "offline"], ["denied", "fresh", "not_authorized"]] as const) {
    const blocked = resource({
      id: `cron_${label}`,
      name: `${label} schedule`,
      kind: "cron",
      source_peer_id: "peer-blocked",
      source_peer: "Blocked peer",
      machine_id: "blocked-machine",
      machine_name: "Blocked machine",
      last_run_status: "running",
      peer_online: freshness !== "offline",
      provenance: { freshness, offline: freshness === "offline" },
      capabilities: {
        get: { allowed: true },
        history: { allowed: true },
        trigger: { allowed: false, unavailable_reason: reason },
        disable: { allowed: false, unavailable_reason: reason },
        update_schedule: { allowed: false, unavailable_reason: reason },
        cancel: { allowed: false, unavailable_reason: reason },
        delete: { allowed: false, unavailable_reason: reason },
      },
    });
    const blockedCalls: string[] = [];
    const { host: blockedHost, root: blockedRoot } = await render(<MeshServiceDrawer state={{ kind: "service", id: blocked.id, sourcePeerId: blocked.source_peer_id }} services={[blocked]} peers={[]} thisMachineName="Local machine" onClose={() => {}} onActionDone={() => {}} actionAdapter={adapter({ trigger: async () => { blockedCalls.push("trigger"); }, setEnabled: async () => { blockedCalls.push("toggle"); }, updateSchedule: async () => { blockedCalls.push("update"); }, cancel: async () => { blockedCalls.push("cancel"); }, remove: async () => { blockedCalls.push("remove"); } })} />);
    assert(blockedHost.querySelector("[data-testid='mesh-operation-unavailable-trigger']")?.textContent?.includes(reason) === true, `${label} capability reason is visible`);
    assert(Array.from(blockedHost.querySelectorAll("button")).filter((button) => /Trigger now|Pause|Cancel running|Remove schedule/.test(button.textContent ?? "")).every((button) => (button as HTMLButtonElement).disabled), `${label} mutations are disabled`);
    assert(blockedCalls.length === 0, `${label} capabilities never invoke mutation adapters`);
    await act(async () => blockedRoot.unmount());
  }
  console.log("✓ Mesh resource actions honor capability authority, returned outcomes, and focus semantics");
}

function DrawerHarness({ service, actionAdapter, onClose }: { service: Service; actionAdapter: MeshResourceActionAdapter; onClose: () => void }) {
  const [state, setState] = useReactState<DrawerState>({ kind: "service", id: service.id, sourcePeerId: service.source_peer_id ?? null });
  return <MeshServiceDrawer state={state} services={[service]} peers={[]} thisMachineName="Local machine" onClose={() => { setState(null); onClose(); }} onActionDone={() => {}} actionAdapter={actionAdapter} />;
}

void main();
