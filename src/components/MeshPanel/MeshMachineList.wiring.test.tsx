import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { CloudReceipt, CloudUsagePayload } from "../../lib/cloudMachines/types";
import { LOCAL_MACHINE_ID } from "../../lib/machinePlacement";
import { plansAction } from "../billing/purchaseSwitch";
import { accountCanvas } from "./accountCanvas";
import { myMachinesView } from "./accountMachines";
import { lenovoRow, macRow, NOW, pair, selfRow, snapshot } from "./accountMachines.testdata";
import { MeshCanvas, type CanvasAccount, type CanvasCloud } from "./MeshCanvas";
import {
  MESH_COLLAPSED_GROUPS_STORAGE_KEY,
  MESH_LIST_SUGGESTION_DISMISSED_KEY,
  MESH_VIEW_MODE_STORAGE_KEY,
} from "./meshViewModeModel";
import type { PeerStatus } from "./useMeshData";

vi.mock("@codeterm/canvas-camera", async (original) => ({
  ...(await original<typeof import("@codeterm/canvas-camera")>()),
  formatLayerTransform: () => "",
  useCanvasViewport: () => ({
    containerRef: { current: null },
    layerRef: { current: null },
    transform: { x: 0, y: 0, scale: 1 },
    handlers: {},
    transformRef: { current: { x: 0, y: 0, scale: 1 } },
    setTransform: () => {},
    fitToView: () => {},
    zoomBy: () => {},
  }),
}));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function makePeer(index: number, patch: Partial<PeerStatus> = {}): PeerStatus {
  return {
    instance_id: `peer-${index}`,
    name: `Machine ${index}`,
    ip: `100.64.0.${index + 1}`,
    api_port: 7685,
    online: true,
    access_level: "full",
    last_seen: new Date((NOW - index * 60) * 1000).toISOString(),
    domios_version: "1.24.0",
    relay_protocol_version: 2,
    ...patch,
  };
}

const USAGE: CloudUsagePayload = {
  used: 9,
  allowance: 40,
  purchased: 0,
  remaining: 31,
  cap: 50,
  burn_per_size: { small: 1, large: 2 },
  currency: "USD",
  sizes: [
    { id: "small", cpu: 2, ram_gb: 4, hourly_rate_cents: 20, credits_per_hour: 1 },
    { id: "large", cpu: 4, ram_gb: 8, hourly_rate_cents: 40, credits_per_hour: 2 },
  ],
};

function makeCloud(id: string, name: string, state: CloudReceipt["state"]): CloudReceipt {
  return {
    kind: "cloud",
    machine_id: id,
    name,
    provider: "fly_machines",
    size: "small",
    state,
    sleep_mode: "stop_billed",
    uptime_secs: state === "awake" ? 900 : 0,
    billing_notice: "",
  };
}

describe("MeshMachineList wiring", () => {
  let host: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    window.localStorage.clear();
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
    window.localStorage.clear();
  });

  it("shows hours and archive progress in the list and opens top up", async () => {
    window.localStorage.setItem(MESH_VIEW_MODE_STORAGE_KEY, "list");
    const machine = makeCloud("archive-progress", "Vault", "asleep");
    const onTopUpMachine = vi.fn();
    const render = async (storage_tier: CloudReceipt["storage_tier"], usage: CloudUsagePayload) => {
      const cloud: CanvasCloud = {
        machines: [{ ...machine, storage_tier }],
        usage,
        isFree: false,
        slotOpen: false,
        pendingCreate: null,
        onOpenSlot: () => {},
        onCloseSlot: () => {},
        onCreateCloud: () => {},
        onPairComputer: () => {},
        onSelectMachine: () => {},
        onTopUpMachine,
        onFailureAction: () => {},
      };
      await act(async () => root.render(
        <MeshCanvas
          thisMachine={{ name: "Studio", ip: "192.168.1.2" }}
          peers={[]}
          onPeerClick={() => {}}
          recentPushes={[]}
          width={1280}
          height={800}
          cloud={cloud}
        />,
      ));
      return host.querySelector<HTMLElement>('[data-testid="mesh-list-row-cloud:archive-progress"]')!;
    };

    const row = await render("evicting", { ...USAGE, warnings: { epoch: 4, pct80_at: 101 } });
    expect(row.textContent).toContain("Low on hours");
    expect(row.textContent).toContain("Archiving…");
    const topUp = Array.from(row.querySelectorAll("button")).find((button) => button.textContent?.trim() === "Top up");
    expect(topUp).toBeDefined();
    await act(async () => topUp!.click());
    expect(onTopUpMachine).toHaveBeenCalledExactlyOnceWith({ ...machine, storage_tier: "evicting" });

    const archived = await render("cold", { ...USAGE, remaining: 0, warnings: { epoch: 4, exhausted_at: 103 } });
    expect(archived.textContent).toContain("Archived");
    expect(archived.textContent).toContain("Out of hours");
  });

  it("persists Canvas | List toggle choices and defaults to List only above 8 machines without a choice", async () => {
    const fourPeers = [1, 2, 3].map((i) => makePeer(i));
    await act(async () => {
      root.render(
        <MeshCanvas
          thisMachine={{ name: "Studio", ip: "192.168.1.2" }}
          peers={fourPeers}
          onPeerClick={() => {}}
          recentPushes={[]}
          width={1280}
          height={800}
        />,
      );
    });
    expect(host.querySelector('[data-testid="mesh-inventory-list"]')).toBeNull();
    const listRadio = host.querySelector<HTMLButtonElement>('[data-testid="mesh-view-toggle-list"]')!;
    expect(listRadio.getAttribute("aria-checked")).toBe("false");

    await act(async () => listRadio.click());
    expect(host.querySelector('[data-testid="mesh-inventory-list"]')).not.toBeNull();
    expect(window.localStorage.getItem(MESH_VIEW_MODE_STORAGE_KEY)).toBe("list");

    await act(async () => root.unmount());
    root = createRoot(host);
    await act(async () => {
      root.render(
        <MeshCanvas
          thisMachine={{ name: "Studio", ip: "192.168.1.2" }}
          peers={fourPeers}
          onPeerClick={() => {}}
          recentPushes={[]}
          width={1280}
          height={800}
        />,
      );
    });
    expect(host.querySelector('[data-testid="mesh-inventory-list"]')).not.toBeNull();

    window.localStorage.clear();
    await act(async () => root.unmount());
    root = createRoot(host);
    const nineMachinesPeers = Array.from({ length: 8 }, (_, i) => makePeer(i + 1));
    await act(async () => {
      root.render(
        <MeshCanvas
          thisMachine={{ name: "Studio", ip: "192.168.1.2" }}
          peers={nineMachinesPeers}
          onPeerClick={() => {}}
          recentPushes={[]}
          width={1280}
          height={800}
        />,
      );
    });
    expect(host.querySelector('[data-testid="mesh-inventory-list"]')).not.toBeNull();

    await act(async () => host.querySelector<HTMLButtonElement>('[data-testid="mesh-view-toggle-canvas"]')!.click());
    expect(host.querySelector('[data-testid="mesh-inventory-list"]')).toBeNull();
    expect(window.localStorage.getItem(MESH_VIEW_MODE_STORAGE_KEY)).toBe("canvas");
  });

  it("shows the bottom suggestion capsule on 8->9 crossing without yanking, respects Keep canvas, and resets at <=6", async () => {
    const renderCount = async (peerCount: number) => {
      const peers = Array.from({ length: peerCount }, (_, i) => makePeer(i + 1));
      await act(async () => {
        root.render(
          <MeshCanvas
            thisMachine={{ name: "Studio", ip: "192.168.1.2" }}
            peers={peers}
            onPeerClick={() => {}}
            recentPushes={[]}
            width={1280}
            height={800}
          />,
        );
      });
    };

    await renderCount(7);
    expect(host.querySelector('[data-testid="mesh-inventory-list"]')).toBeNull();
    expect(host.querySelector('[data-testid="mesh-list-suggestion"]')).toBeNull();

    await renderCount(8);
    expect(host.querySelector('[data-testid="mesh-inventory-list"]')).toBeNull();
    const capsule = host.querySelector('[data-testid="mesh-list-suggestion"]');
    expect(capsule).not.toBeNull();
    expect(capsule!.textContent).toContain("9 machines now.");

    const keepBtn = Array.from(capsule!.querySelectorAll("button")).find((b) => b.textContent?.trim() === "Keep canvas")!;
    await act(async () => keepBtn.click());
    expect(host.querySelector('[data-testid="mesh-list-suggestion"]')).toBeNull();
    expect(window.localStorage.getItem(MESH_LIST_SUGGESTION_DISMISSED_KEY)).toBe("1");

    await renderCount(7);
    await renderCount(8);
    expect(host.querySelector('[data-testid="mesh-list-suggestion"]')).toBeNull();

    await renderCount(5);
    expect(window.localStorage.getItem(MESH_LIST_SUGGESTION_DISMISSED_KEY)).toBe("0");
    await renderCount(8);
    const secondCapsule = host.querySelector('[data-testid="mesh-list-suggestion"]');
    expect(secondCapsule).not.toBeNull();

    const switchBtn = Array.from(secondCapsule!.querySelectorAll("button")).find((b) => b.textContent?.trim() === "Switch to list")!;
    await act(async () => switchBtn.click());
    expect(host.querySelector('[data-testid="mesh-inventory-list"]')).not.toBeNull();
    expect(host.querySelector('[data-testid="mesh-list-suggestion"]')).toBeNull();
  });

  it("wires every list row action to the same canvas handlers, persists group collapse, and supports keyboard navigation", async () => {
    const onPeerClick = vi.fn();
    const onSelectMachine = vi.fn();
    const onSleepMachine = vi.fn(async () => ({ ok: true }));
    const onWakeMachine = vi.fn(async () => ({ ok: true }));
    const onTopUpMachine = vi.fn();
    const onSignIn = vi.fn();
    const onPlans = vi.fn();
    const onRemove = vi.fn(async () => true);
    const onRename = vi.fn(async (_row: unknown, _name: string) => true);

    const signedOutRow = { ...lenovoRow, installation_id: "inst-so", display_name: "Mini", presence: "signed_out" as const };
    const overLimitRow = { ...lenovoRow, installation_id: "inst-limit", display_name: "laptop-2", eligibility: "mesh-allowance" as const };
    const offlineRows = [1, 2, 3, 4].map((i) => ({
      ...lenovoRow,
      installation_id: `inst-off-${i}`,
      display_name: `Offline ${i}`,
      endpoint: null,
      authenticated_at: NOW - i * 3600,
    }));

    const state = snapshot({
      registry: {
        observed_at: NOW,
        rows: [selfRow, macRow, signedOutRow, overLimitRow, ...offlineRows],
      },
      pairs: [pair("peer-mac", "MacBook Air", { installation_id: macRow.installation_id, online: true })],
    });
    const peers = [makePeer(1, { instance_id: "peer-mac", name: "MacBook Air", online: true })];
    const canvas = accountCanvas({
      view: myMachinesView({ account: "signed_in", snapshot: state, now: NOW }),
      peers,
      loadFailed: false,
      listFailed: false,
      failures: new Map(),
    });
    const account: CanvasAccount = {
      selfName: "Studio",
      canvas,
      busy: null,
      signingIn: false,
      now: NOW * 1000,
      actions: { onPlans, onDismiss: () => {}, onRemove, onRename },
      onSignIn,
      onRetry: () => {},
    };
    const cloudMachines = [
      makeCloud("cm-run", "fly-builder", "awake"),
      makeCloud("cm-sleep", "fly-scratch", "asleep"),
      makeCloud("cm-ooc", "fly-render", "out_of_credits"),
    ];
    const cloud: CanvasCloud = {
      machines: cloudMachines,
      usage: USAGE,
      isFree: false,
      slotOpen: false,
      pendingCreate: null,
      onOpenSlot: () => {},
      onCloseSlot: () => {},
      onCreateCloud: () => {},
      onPairComputer: () => {},
      onSelectMachine,
      onTopUpMachine,
      onSleepMachine,
      onWakeMachine,
      onFailureAction: () => {},
    };

    await act(async () => {
      root.render(
        <MeshCanvas
          account={account}
          cloud={cloud}
          thisMachine={{ name: "Studio", ip: "192.168.1.2" }}
          peers={peers}
          onPeerClick={onPeerClick}
          recentPushes={[]}
          width={1280}
          height={800}
        />,
      );
    });

    expect(host.querySelector('[data-testid="mesh-inventory-list"]')).not.toBeNull();
    expect(host.textContent).toContain("31 h left this month");

    const offlineGroup = host.querySelector('[data-testid="mesh-group-offline"]')!;
    expect(offlineGroup.querySelector('[role="list"]')).toBeNull();
    const showOffline = Array.from(offlineGroup.querySelectorAll("button")).find((b) => b.textContent?.trim() === "Show")!;
    await act(async () => showOffline.click());
    expect(offlineGroup.querySelector('[role="list"]')).not.toBeNull();
    expect(JSON.parse(window.localStorage.getItem(MESH_COLLAPSED_GROUPS_STORAGE_KEY)!)).toEqual({ offline: false });

    const selfRowEl = host.querySelector<HTMLElement>('[data-testid="mesh-list-row-__self__"]')!;
    selfRowEl.focus();
    await act(async () => selfRowEl.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true })));
    expect(onPeerClick).toHaveBeenCalledWith(LOCAL_MACHINE_ID);

    await act(async () => selfRowEl.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true })));
    const macRowEl = host.querySelector<HTMLElement>('[data-testid="mesh-list-row-peer-mac"]')!;
    expect(document.activeElement).toBe(macRowEl);

    await act(async () => macRowEl.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true })));
    expect(onPeerClick).toHaveBeenCalledWith("peer-mac");

    await act(async () => macRowEl.dispatchEvent(new KeyboardEvent("keydown", { key: "ContextMenu", bubbles: true })));
    const removeItem = Array.from(document.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')).find(
      (item) => item.textContent?.trim() === "Remove from mesh",
    );
    expect(removeItem).toBeDefined();
    await act(async () => removeItem!.click());
    const confirmRemove = Array.from(host.querySelectorAll<HTMLButtonElement>("button")).find(
      (btn) => btn.textContent?.trim() === "Confirm remove",
    )!;
    await act(async () => confirmRemove.click());
    expect(onRemove).toHaveBeenCalledTimes(1);

    const builderRow = host.querySelector<HTMLElement>('[data-testid="mesh-list-row-cloud:cm-run"]')!;
    await act(async () => builderRow.click());
    expect(onSelectMachine).toHaveBeenCalledWith("cm-run");
    const sleepBtn = Array.from(builderRow.querySelectorAll("button")).find((b) => b.textContent?.trim() === "Sleep")!;
    await act(async () => sleepBtn.click());
    expect(onSleepMachine).toHaveBeenCalledWith("cm-run");

    const scratchRow = host.querySelector<HTMLElement>('[data-testid="mesh-list-row-cloud:cm-sleep"]')!;
    const wakeBtn = Array.from(scratchRow.querySelectorAll("button")).find((b) => b.textContent?.trim() === "Wake")!;
    await act(async () => wakeBtn.click());
    expect(onWakeMachine).toHaveBeenCalledWith("cm-sleep");

    const renderRow = host.querySelector<HTMLElement>('[data-testid="mesh-list-row-cloud:cm-ooc"]')!;
    const topUpBtn = Array.from(renderRow.querySelectorAll("button")).find((b) => b.textContent?.trim() === "Top up")!;
    await act(async () => topUpBtn.click());
    expect(onTopUpMachine).toHaveBeenCalledWith(cloudMachines[2]);

    const miniRow = host.querySelector<HTMLElement>('[data-testid="mesh-list-row-account:inst-so"]')!;
    const signInBtn = Array.from(miniRow.querySelectorAll("button")).find((b) => b.textContent?.trim() === "Sign in")!;
    await act(async () => signInBtn.click());
    expect(onSignIn).toHaveBeenCalledTimes(1);

    const limitRow = host.querySelector<HTMLElement>('[data-testid="mesh-list-row-account:inst-limit"]')!;
    const expectedPlansLabel = plansAction("See plans");
    const plansBtn = Array.from(limitRow.querySelectorAll("button")).find(
      (b) => b.textContent?.trim() === expectedPlansLabel,
    )!;
    await act(async () => plansBtn.click());
    expect(onPlans).toHaveBeenCalledWith("inst-limit");

    const renameBtn = macRowEl.querySelector<HTMLButtonElement>('button[aria-label="Rename MacBook"]')!;
    await act(async () => renameBtn.click());
    const nameInput = macRowEl.querySelector<HTMLInputElement>('input[aria-label="Name for MacBook"]')!;
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(nameInput, "Travel Air");
      nameInput.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await act(async () => nameInput.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, code: "Enter" })));
    expect(onRename).toHaveBeenCalledTimes(1);
    expect(onRename.mock.calls[0][1]).toBe("Travel Air");
  });

  it("preserves scroll position on click and shows the filter field above 20 machines", async () => {
    const onPeerClick = vi.fn();
    const peers = Array.from({ length: 30 }, (_, i) => makePeer(i + 1, { online: true }));
    await act(async () => {
      root.render(
        <MeshCanvas
          thisMachine={{ name: "Studio", ip: "192.168.1.2" }}
          peers={peers}
          onPeerClick={onPeerClick}
          recentPushes={[]}
          width={1280}
          height={600}
        />,
      );
    });

    const scroller = host.querySelector<HTMLElement>('[data-testid="mesh-inventory-list"]')!;
    expect(host.querySelector('[data-testid="mesh-list-filter"]')).not.toBeNull();
    scroller.scrollTop = 420;

    const targetRow = host.querySelector<HTMLElement>('[data-testid="mesh-list-row-peer-15"]')!;
    await act(async () => targetRow.click());
    expect(onPeerClick).toHaveBeenCalledWith("peer-15");
    expect(scroller.scrollTop).toBe(420);
  });

  it("excludes live sleeping, cold, and draining cloud machines from stale cleanup count and explains cloud exclusion in confirmation", async () => {
    const onRemoveStale = vi.fn(async (ids: string[]) => ({
      dry_run: false,
      matched: ids.length,
      removed: ids.length,
      already_removed: 0,
      skipped: 0,
      rows: [],
    }));

    const ordinaryStale = {
      ...lenovoRow,
      installation_id: "inst-ordinary-stale",
      display_name: "fly-ordinary-laptop",
      membership_expires_at: NOW - 3600,
      authenticated_at: NOW - 7200,
    };
    const asleepCloudRow = {
      ...lenovoRow,
      installation_id: "880cbbeedc6f433f8f332c9af20bc8c8",
      display_name: "quill-fly-124-warm",
      membership_expires_at: NOW - 3600,
      authenticated_at: NOW - 7200,
    };
    const coldCloudRow = {
      ...lenovoRow,
      installation_id: "inst-cloud-cold",
      display_name: "vault-box",
      membership_expires_at: NOW - 86_400 * 30,
      authenticated_at: NOW - 86_400 * 30,
    };
    const drainingCloudRow = {
      ...lenovoRow,
      installation_id: "inst-cloud-draining",
      display_name: "worker-drain",
      membership_expires_at: NOW - 3600,
      authenticated_at: NOW - 7200,
    };

    const state = snapshot({
      registry: {
        observed_at: NOW,
        rows: [selfRow, macRow, ordinaryStale, asleepCloudRow, coldCloudRow, drainingCloudRow],
      },
      pairs: [pair("peer-mac", "MacBook Air", { installation_id: macRow.installation_id, online: true })],
    });
    const peers = [makePeer(1, { instance_id: "peer-mac", name: "MacBook Air", online: true })];
    const canvas = accountCanvas({
      view: myMachinesView({ account: "signed_in", snapshot: state, now: NOW }),
      peers,
      loadFailed: false,
      listFailed: false,
      failures: new Map(),
    });
    const account: CanvasAccount = {
      selfName: "Studio",
      canvas,
      busy: null,
      signingIn: false,
      now: NOW * 1000,
      actions: {
        onPlans: () => {},
        onDismiss: () => {},
        onRemove: async () => true,
        onRemoveStale,
      },
      onSignIn: () => {},
      onRetry: () => {},
    };
    const cloud: CanvasCloud = {
      machines: [
        {
          ...makeCloud("cm-asleep", "quill-fly-124-warm", "asleep"),
          installation_id: "880cbbeedc6f433f8f332c9af20bc8c8",
          storage_tier: "hot",
        },
        {
          ...makeCloud("cm-cold", "vault-box", "asleep"),
          installation_id: "inst-cloud-cold",
          storage_tier: "cold",
        },
        {
          ...makeCloud("cm-draining", "worker-drain", "draining"),
          installation_id: "inst-cloud-draining",
          storage_tier: "hot",
        },
      ],
      usage: USAGE,
      isFree: false,
      slotOpen: false,
      pendingCreate: null,
      onOpenSlot: () => {},
      onCloseSlot: () => {},
      onCreateCloud: () => {},
      onPairComputer: () => {},
      onSelectMachine: () => {},
      onTopUpMachine: () => {},
      onSleepMachine: async () => ({ ok: true }),
      onWakeMachine: async () => ({ ok: true }),
      onFailureAction: () => {},
    };

    window.localStorage.setItem(MESH_VIEW_MODE_STORAGE_KEY, "list");
    await act(async () => {
      root.render(
        <MeshCanvas
          account={account}
          cloud={cloud}
          thisMachine={{ name: "Studio", ip: "192.168.1.2" }}
          peers={peers}
          onPeerClick={() => {}}
          recentPushes={[]}
          width={1280}
          height={800}
        />,
      );
    });

    const cleanupButton = host.querySelector<HTMLButtonElement>('[data-testid="mesh-stale-cleanup"]')!;
    expect(cleanupButton).not.toBeNull();
    expect(cleanupButton.textContent?.trim()).toBe("Clean up 1 stale machine");

    await act(async () => cleanupButton.click());
    const confirmGroup = host.querySelector<HTMLElement>('[role="group"][aria-label="Remove 1 stale machine"]')!;
    expect(confirmGroup).not.toBeNull();
    expect(confirmGroup.textContent).toContain("fly-ordinary-laptop");
    expect(confirmGroup.textContent).not.toContain("quill-fly-124-warm");
    expect(confirmGroup.textContent).not.toContain("vault-box");
    expect(confirmGroup.textContent).not.toContain("worker-drain");
    const exclusionNote = confirmGroup.querySelector('[data-testid="mesh-stale-cloud-exclusion"]');
    expect(exclusionNote?.textContent).toContain(
      "Cloud machines are never removed here; delete them from the cloud drawer",
    );

    const confirmBtn = Array.from(confirmGroup.querySelectorAll<HTMLButtonElement>("button")).find(
      (btn) => btn.textContent?.trim() === "Remove 1 machine",
    )!;
    await act(async () => confirmBtn.click());
    expect(onRemoveStale).toHaveBeenCalledTimes(1);
    expect(onRemoveStale).toHaveBeenCalledWith(["inst-ordinary-stale"]);
  });

  it("hides stale cleanup when only live cloud machines look stale and omits the cloud note when none are excluded", async () => {
    const asleepCloudRow = {
      ...lenovoRow,
      installation_id: "880cbbeedc6f433f8f332c9af20bc8c8",
      display_name: "quill-fly-124-warm",
      membership_expires_at: NOW - 3600,
      authenticated_at: NOW - 7200,
    };
    const overLimitRow = {
      ...lenovoRow,
      installation_id: "inst-limit",
      display_name: "Locked Laptop",
      eligibility: "mesh-allowance" as const,
    };
    const cloudOnlyState = snapshot({
      registry: { observed_at: NOW, rows: [selfRow, asleepCloudRow, overLimitRow] },
      pairs: [],
    });
    const cloudOnlyCanvas = accountCanvas({
      view: myMachinesView({ account: "signed_in", snapshot: cloudOnlyState, now: NOW }),
      peers: [],
      loadFailed: false,
      listFailed: false,
      failures: new Map(),
    });
    const makeAccount = (canvas: ReturnType<typeof accountCanvas>): CanvasAccount => ({
      selfName: "Studio",
      canvas,
      busy: null,
      signingIn: false,
      now: NOW * 1000,
      actions: {
        onPlans: () => {},
        onDismiss: () => {},
        onRemove: async () => true,
        onRemoveStale: async () => ({
          dry_run: false,
          matched: 0,
          removed: 0,
          already_removed: 0,
          skipped: 0,
          rows: [],
        }),
      },
      onSignIn: () => {},
      onRetry: () => {},
    });
    const cloud: CanvasCloud = {
      machines: [
        {
          ...makeCloud("cm-asleep", "quill-fly-124-warm", "asleep"),
          installation_id: "880cbbeedc6f433f8f332c9af20bc8c8",
          storage_tier: "hot",
        },
      ],
      usage: USAGE,
      isFree: false,
      slotOpen: false,
      pendingCreate: null,
      onOpenSlot: () => {},
      onCloseSlot: () => {},
      onCreateCloud: () => {},
      onPairComputer: () => {},
      onSelectMachine: () => {},
      onTopUpMachine: () => {},
      onSleepMachine: async () => ({ ok: true }),
      onWakeMachine: async () => ({ ok: true }),
      onFailureAction: () => {},
    };

    window.localStorage.setItem(MESH_VIEW_MODE_STORAGE_KEY, "list");
    await act(async () => {
      root.render(
        <MeshCanvas
          account={makeAccount(cloudOnlyCanvas)}
          cloud={cloud}
          thisMachine={{ name: "Studio", ip: "192.168.1.2" }}
          peers={[]}
          onPeerClick={() => {}}
          recentPushes={[]}
          width={1280}
          height={800}
        />,
      );
    });
    expect(host.querySelector('[data-testid="mesh-stale-cleanup"]')).toBeNull();

    // Without any cloud machines excluded, confirmation omits the cloud drawer note.
    const ordinaryOnlyRow = {
      ...lenovoRow,
      installation_id: "inst-ordinary",
      display_name: "Old Mac",
      presence: "signed_out" as const,
      authenticated_at: NOW - 3600,
    };
    const ordinaryState = snapshot({
      registry: { observed_at: NOW, rows: [selfRow, ordinaryOnlyRow] },
      pairs: [],
    });
    const ordinaryCanvas = accountCanvas({
      view: myMachinesView({ account: "signed_in", snapshot: ordinaryState, now: NOW }),
      peers: [],
      loadFailed: false,
      listFailed: false,
      failures: new Map(),
    });
    await act(async () => {
      root.render(
        <MeshCanvas
          account={makeAccount(ordinaryCanvas)}
          thisMachine={{ name: "Studio", ip: "192.168.1.2" }}
          peers={[]}
          onPeerClick={() => {}}
          recentPushes={[]}
          width={1280}
          height={800}
        />,
      );
    });
    const btn = host.querySelector<HTMLButtonElement>('[data-testid="mesh-stale-cleanup"]')!;
    expect(btn.textContent?.trim()).toBe("Clean up 1 stale machine");
    await act(async () => btn.click());
    expect(host.querySelector('[data-testid="mesh-stale-cloud-exclusion"]')).toBeNull();
  });
});
