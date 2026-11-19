import { useCallback, useEffect, useRef, useState, type KeyboardEvent as ReactKeyboardEvent } from "react";
import { ChevronDown } from "lucide-react";
import { listen } from "../../lib/bridge";
import { rankPeersByLastActive } from "../../lib/peerRanking";
import {
  localJson,
} from "../../lib/webApi";
import { useWorkspaceBrowseTarget } from "../../lib/workspaceBrowseTarget";
import { setSidebarMachineFilter, useSidebarMachineFilter } from "../Sidebar/sidebarMachineFilter";
import { AnchoredPopover } from "../shared/AnchoredPopover";
import { MachineGlyph } from "../shared/MachineGlyph";
import { cleanMachineName, machineLabel, type MeshIdentityProjection } from "../../lib/machineIdentity";
import { menuKeyAction } from "./menuKeyboard";
import { RELAY_PROTOCOL_VERSION } from "../../lib/types";
import { hasLocalInstanceName, localInstanceLabel, onMeshPeersLoaded, rememberLocalInstanceName } from "../../lib/localInstanceName";

type PairedPeer = Pick<import("../../../gen/PairedPeer").PairedPeer, "instance_id" | "instance_name" | "addresses" | "api_port" | "online" | "relay_protocol_version" | "last_active_ms">;
type MeshPeers = Omit<Pick<import("../../../gen/MeshPeersResponse").MeshPeersResponse, "paired" | "local_instance_name">, "paired"> & {paired: PairedPeer[]} & MeshIdentityProjection;

/** Selects which paired host supplies the Web workspace inventory. */
export function WebPeerPicker({ compact = false }: { compact?: boolean }) {
  const [peers, setPeers] = useState<PairedPeer[]>([]);
  const [localName, setLocalName] = useState(localInstanceLabel);
  const [open, setOpen] = useState(false);
  const target = useWorkspaceBrowseTarget();
  const filter = useSidebarMachineFilter();
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

  const selectedPeer = peers.find(peer => peer.instance_id === target?.id);
  const label = target ? cleanMachineName(selectedPeer?.instance_name ?? target.name, target.id) ?? "Machine" : localName;
  const identity = target ? identities.machine_identities?.[target.id] : identities.local_machine_identity;

  const switchTo = useCallback(
    (selection: PairedPeer | "all" | "local") => {
      setSidebarMachineFilter(typeof selection === "string" ? selection : {
        id: selection.instance_id,
        name: cleanMachineName(selection.instance_name, selection.instance_id) ?? "Machine",
      });
      closeMenu();
    },
    [closeMenu],
  );

  return (
    <div ref={rootRef} className="relative" data-testid="peer-picker">
      <button
        ref={triggerRef}
        type="button"
        data-testid="peer-picker-button"
        aria-label={`Filter sidebar by machine, ${filter === "all" ? "All machines, " : ""}${machineLabel(label, identity)}`}
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => (open ? closeMenu() : setOpen(true))}
        data-filtered={filter !== "all" || undefined}
        className={`ct-bar-pill ct-machine-switcher ${compact ? "min-w-0 max-w-[min(10rem,40vw)] sm:max-w-none" : ""}`}
        title="Filter sidebar by machine"
      >
        <MachineGlyph {...identity} />
        <span className="min-w-0 truncate">{label}</span>
        <ChevronDown size={12} className="shrink-0 opacity-60" aria-hidden="true" />
      </button>
      <AnchoredPopover open={open} anchor={rootRef} placement={compact ? "bottom-end" : "bottom-start"} onClose={dismissMenu}>
        <div
          ref={menuRef}
          data-testid="peer-picker-menu"
          role="menu"
          aria-label="Machines"
          onKeyDown={onMenuKeyDown}
          className="ct-menu ct-peer-menu w-[320px]"
        >
          <button type="button" role="menuitem" data-testid="peer-picker-all"
            aria-current={filter === "all" ? "true" : undefined}
            onClick={() => switchTo("all")} className="ct-menu-row ct-machine-menu-row">All machines</button>
          <button type="button" role="menuitem" data-testid="peer-picker-self"
            aria-current={filter === "local" ? "true" : undefined}
            onClick={() => switchTo("local")} className="ct-menu-row ct-machine-menu-row">This machine</button>
          {peers.map((p) => {
            const active = target?.id === p.instance_id;
            const name = cleanMachineName(p.instance_name, p.instance_id) ?? "Machine";
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
                aria-label={machineLabel(name, identity, status)}
                data-peer-id={p.instance_id}
                data-peer-name={name}
                data-peer-online={String(!!p.online)}
                onClick={() => switchTo(p)}
                className="ct-menu-row ct-machine-menu-row min-h-[44px] sm:min-h-0"
                title={[name, status, endpoint && `${endpoint}:${p.api_port}`].filter(Boolean).join(" · ")}
              >
                <span className="ct-menu-row__icon">
                  <MachineGlyph {...identity} size="mark-15" />
                </span>
                <span className="min-w-0 flex-1 truncate">{name}</span>
                <HostStatus status={status} />
              </button>
            );
          })}
        </div>
      </AnchoredPopover>
    </div>
  );
}

function HostStatus({ status }: { status: string }) {
  const tone = status === "online" ? "var(--ct-green)" : status === "update required" ? "var(--ct-yellow)" : "var(--ct-muted)";
  return (
    <span className="flex w-24 shrink-0 items-center gap-1.5 whitespace-nowrap text-[length:var(--ct-font-size-meta)] text-[var(--ct-muted-fg)]">
      <span aria-hidden="true" className="size-2 rounded-full" style={{ backgroundColor: tone }} />
      {status}
    </span>
  );
}
