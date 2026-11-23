import { useCallback, useContext, useEffect, useRef, useState, type CSSProperties, type KeyboardEvent as ReactKeyboardEvent } from "react";
import { ChevronDown, Network } from "lucide-react";
import { listen } from "../../lib/bridge";
import { rankPeersByLastActive } from "../../lib/peerRanking";
import {
  localJson,
} from "../../lib/webApi";
import { useWorkspaceBrowseTarget } from "../../lib/workspaceBrowseTarget";
import { setSidebarMachineFilter, useSidebarMachineFilter } from "../Sidebar/sidebarMachineFilter";
import { AnchoredPopover } from "../shared/AnchoredPopover";
import { MachineGlyph } from "../shared/MachineGlyph";
import { MachinePickerRow, MachineThisBadge, machineRowLabel } from "../shared/MachineRow";
import { cleanMachineName, machineColor, machineLabel, type MachineIdentity, type MeshIdentityProjection } from "../../lib/machineIdentity";
import { MachineAppearanceContext } from "../../lib/useMachineAppearance";
import { MachineAppearanceActions, MachineCopyNotice, MachineWallpaperSwatch, NowOnMachine, useMachineAppearanceCopy } from "./MachineWallpaperDetails";
import { useAccountMachineNames } from "../../lib/useMeshMachineIdentities";
import { menuKeyAction } from "./menuKeyboard";
import { RELAY_PROTOCOL_VERSION } from "../../lib/types";
import { hasLocalInstanceName, localInstanceLabel, onMeshPeersLoaded, rememberLocalInstanceName } from "../../lib/localInstanceName";

type PairedPeer = Pick<import("../../../gen/PairedPeer").PairedPeer, "instance_id" | "instance_name" | "addresses" | "api_port" | "online" | "relay_protocol_version" | "last_active_ms">;
type MeshPeers = Omit<Pick<import("../../../gen/MeshPeersResponse").MeshPeersResponse, "paired" | "local_instance_name">, "paired"> & {paired: PairedPeer[]} & MeshIdentityProjection;

/** Selects which paired host supplies the Web workspace inventory. */
export function WebPeerPicker({ compact = false, onOpenWallpaper }: { compact?: boolean; onOpenWallpaper?: () => void }) {
  const [peers, setPeers] = useState<PairedPeer[]>([]);
  const [localName, setLocalName] = useState(localInstanceLabel);
  const [open, setOpen] = useState(false);
  const [notice, setNotice] = useState<{ label: string; key: number }>();
  const [previousPill, setPreviousPill] = useState<{ label: string; all: boolean; local: boolean; identity?: MachineIdentity; key: number }>();
  const noticeSequence = useRef(0);
  const appearance = useContext(MachineAppearanceContext);
  const copy = useMachineAppearanceCopy();
  const target = useWorkspaceBrowseTarget();
  const filter = useSidebarMachineFilter();
  const accountNames = useAccountMachineNames();
  const [identities, setIdentities] = useState<MeshIdentityProjection>({});
  const rootRef = useRef<HTMLDivElement | null>(null);
  const menuRef = useRef<HTMLDivElement | null>(null);
  const triggerRef = useRef<HTMLButtonElement | null>(null);

  const refresh = useCallback(async () => {
    try {
      const res = await localJson<MeshPeers>("GET", "/api/mesh/peers");
      const paired = res.paired ?? [];
      setLocalName(rememberLocalInstanceName(res.local_instance_name));
      setPeers(rankPeersByLastActive(paired));
      setIdentities({ local_machine_identity: res.local_machine_identity, machine_identities: res.machine_identities });
    } catch {
      // Peer list is a convenience — keep the last known list on errors.
      if (hasLocalInstanceName()) return;
      try {
        const health = await localJson<{ instance?: { display_name?: string } }>("GET", "/api/health");
        setLocalName(rememberLocalInstanceName(health.instance?.display_name));
      } catch {
        // Neither source answered; the label stays on the generic fallback.
      }
    }
  }, []);

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;
    let stopIdentity: (() => void) | undefined;
    const stopPeersLoaded = onMeshPeersLoaded(() => void refresh());
    void refresh();
    void listen("mesh-state-changed", () => void refresh()).then((registered) => {
      if (active) {
        unlisten = registered;
        void refresh();
      } else {
        registered();
      }
    }).catch(() => undefined);
    void listen("mesh-identity-changed", () => void refresh()).then(registered => {
      if (active) stopIdentity = registered;
      else registered();
    }).catch(() => undefined);
    return () => {
      active = false;
      stopPeersLoaded();
      unlisten?.();
      stopIdentity?.();
    };
  }, [refresh]);

  const menuItems = useCallback(
    () => Array.from(menuRef.current?.querySelectorAll<HTMLElement>('[role="menuitem"]:not([disabled])') ?? []),
    [],
  );

  const closeMenu = useCallback(() => {
    setOpen(false);
    triggerRef.current?.focus();
  }, []);
  const dismissMenu = useCallback(() => setOpen(false), []);

  // An explicitly opened menu takes focus so arrows and Escape reach it.
  useEffect(() => {
    if (open) menuItems()[0]?.focus();
  }, [open, menuItems]);

  const onMenuKeyDown = useCallback(
    (event: ReactKeyboardEvent<HTMLDivElement>) => {
      const items = menuItems();
      const action = menuKeyAction(event.key, items.indexOf(document.activeElement as HTMLElement), items.length);
      if (!action) return;
      event.preventDefault();
      if (action.kind === "close") closeMenu();
      else items[action.index]?.focus();
    },
    [closeMenu, menuItems],
  );

  const showAll = filter === "all";
  const allMachinesLabel = "All machines";
  const selectedPeer = peers.find(peer => peer.instance_id === target?.id);
  const label = target ? accountNames.peers.get(target.id) ?? cleanMachineName(selectedPeer?.instance_name ?? target.name, target.id) ?? "Machine" : accountNames.local ?? localName;
  const identity = target ? identities.machine_identities?.[target.id] : identities.local_machine_identity;
  const localMachine = { name: accountNames.local ?? localName, identity: identities.local_machine_identity, status: "online", local: true };
  const pillLabel = showAll ? allMachinesLabel : filter === "local" ? machineRowLabel(localMachine) : machineLabel(label, identity);
  const source = { name: label, peerId: appearance?.peerId ?? target?.id, available: appearance?.ready ?? !target };
  const copyTargets = [
    { name: localMachine.name, peerId: undefined, available: true },
    ...peers.map(peer => ({
      name: accountNames.peers.get(peer.instance_id) ?? cleanMachineName(peer.instance_name, peer.instance_id) ?? "Machine",
      peerId: peer.instance_id,
      available: peer.online === true && peer.relay_protocol_version === RELAY_PROTOCOL_VERSION,
      unavailableReason: peer.online ? "not applied, update required" : undefined,
    })),
  ].filter(machine => machine.peerId !== source.peerId);

  function switchTo(selection: PairedPeer | "all" | "local") {
      const changed = typeof selection === "string" ? selection !== filter : target?.id !== selection.instance_id;
      if (changed) {
        setPreviousPill({ label: showAll ? allMachinesLabel : label, all: showAll, local: filter === "local", identity, key: ++noticeSequence.current });
      }
      if (typeof selection === "string") {
        if (selection === "all") setNotice(undefined);
        else if (changed) setNotice({ key: noticeSequence.current, label: localMachine.name });
        setSidebarMachineFilter(selection);
      } else {
        const name = accountNames.peers.get(selection.instance_id) ?? cleanMachineName(selection.instance_name, selection.instance_id) ?? "Machine";
        if (changed) setNotice({ key: noticeSequence.current, label: name });
        setSidebarMachineFilter({ id: selection.instance_id, name });
      }
      closeMenu();
  }
  const visiblePill = previousPill ?? { label: showAll ? allMachinesLabel : label, all: showAll, local: filter === "local", identity };

  return (
    <div ref={rootRef} className="relative" data-testid="peer-picker">
      <button
        ref={triggerRef}
        type="button"
        data-testid="peer-picker-button"
        aria-label={`Filter sidebar by machine, ${pillLabel}`}
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => (open ? closeMenu() : setOpen(true))}
        data-filtered={!showAll || undefined}
        className={`ct-bar-pill ct-machine-switcher ${compact ? "min-w-0 max-w-[min(10rem,40vw)] sm:max-w-none" : ""}`}
        title="Filter sidebar by machine"
      >
        {visiblePill.all ? <Network size={12} className="shrink-0" aria-hidden="true" /> : <MachineGlyph {...visiblePill.identity} />}
        <span className="min-w-0 truncate">{visiblePill.label}</span>
        {visiblePill.local && <MachineThisBadge className="hidden min-[400px]:inline-flex" />}
        {previousPill && <span key={previousPill.key} aria-hidden="true" className="ct-machine-label-transition"
          onAnimationEnd={() => setPreviousPill(current => current?.key === previousPill.key ? undefined : current)} />}
        <ChevronDown size={12} className="shrink-0 opacity-60" aria-hidden="true" />
      </button>
      {notice && <NowOnMachine key={notice.key} label={notice.label} onDismiss={() => setNotice(undefined)} />}
      <MachineCopyNotice copy={copy} />
      <AnchoredPopover open={open} anchor={rootRef} placement={compact ? "bottom-end" : "bottom-start"} onClose={dismissMenu}>
        <div
          ref={menuRef}
          data-testid="peer-picker-menu"
          role="menu"
          aria-label="Machines"
          onKeyDown={onMenuKeyDown}
          data-wallpaper-tint={Boolean(identity) || undefined}
          style={{ "--ct-wallpaper-identity": `var(--ct-machine-${machineColor(identity?.color)})` } as CSSProperties}
          className="ct-menu ct-menu--opaque ct-peer-menu w-max max-w-[min(24rem,calc(100vw-2rem))]"
        >
          <button type="button" role="menuitem" data-testid="peer-picker-all"
            aria-current={showAll ? "true" : undefined}
            onClick={() => switchTo("all")} className="ct-menu-row ct-machine-menu-row">
            <span className="flex min-w-0 items-center gap-2">
              <Network size={15} className="shrink-0" aria-hidden="true" />
              <span className="truncate">{allMachinesLabel}</span>
            </span>
          </button>
          <button type="button" role="menuitem" data-testid="peer-picker-self"
            aria-current={filter === "local" ? "true" : undefined}
            aria-label={machineRowLabel(localMachine)} onClick={() => switchTo("local")}
            className="ct-menu-row ct-machine-menu-row">
            <MachinePickerRow {...localMachine} trailing={open && <MachineWallpaperSwatch machine={{
              name: localMachine.name, available: true,
            }} identity={localMachine.identity} preview={source.peerId === undefined ? appearance?.appearance : undefined} />} />
          </button>
          {peers.map((p) => {
            const active = target?.id === p.instance_id;
            const name = accountNames.peers.get(p.instance_id) ?? cleanMachineName(p.instance_name, p.instance_id) ?? "Machine";
            const compatible = p.relay_protocol_version === RELAY_PROTOCOL_VERSION;
            const status = p.online ? compatible ? "online" : "update required" : "offline";
            const identity = identities.machine_identities?.[p.instance_id];
            const endpoint = p.addresses?.[0]?.ip;
            return (
              <button
                type="button"
                role="menuitem"
                key={p.instance_id}
                data-testid="peer-picker-item"
                aria-current={active ? "true" : undefined}
                aria-label={machineRowLabel({ name, identity, status })}
                data-peer-id={p.instance_id}
                data-peer-name={name}
                data-peer-online={String(!!p.online)}
                onClick={() => switchTo(p)}
                className="ct-menu-row ct-machine-menu-row min-h-[44px] sm:min-h-0"
                title={[name, status, endpoint && `${endpoint}:${p.api_port}`].filter(Boolean).join(" · ")}
              >
                <MachinePickerRow name={name} identity={identity} status={status} trailing={open && <MachineWallpaperSwatch machine={{
                  name, peerId: p.instance_id, available: Boolean(p.online && compatible),
                }} identity={identity} preview={active ? appearance?.appearance : undefined} />} />
              </button>
            );
          })}
          {appearance && <MachineAppearanceActions key={source.peerId ?? "local"} source={source} targets={copyTargets} copy={copy}
            onAsk={onOpenWallpaper ? () => { closeMenu(); onOpenWallpaper(); } : undefined} />}
        </div>
      </AnchoredPopover>
    </div>
  );
}
