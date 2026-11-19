import { useCallback, useEffect, useRef, useState, type KeyboardEvent as ReactKeyboardEvent } from "react";
import { Check, ChevronDown } from "lucide-react";
import { listen } from "../../lib/bridge";
import { formatLastActiveBadge, rankPeersByLastActive } from "../../lib/peerRanking";
import {
  localJson,
  relayPeerUsable,
  selfHost,
} from "../../lib/webApi";
import { setWorkspaceBrowseTarget, useWorkspaceBrowseTarget } from "../../lib/workspaceBrowseTarget";
import { AnchoredPopover } from "../shared/AnchoredPopover";
import { MachineGlyph } from "../shared/MachineGlyph";
import { machineLabel, type MeshIdentityProjection } from "../../lib/machineIdentity";
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

  const onSelf = !target;
  const label = onSelf ? localName : target.name;
  const identity = onSelf ? identities.local_machine_identity : identities.machine_identities?.[target.id];

  const switchTo = useCallback(
    (peer: PairedPeer | null) => {
      setWorkspaceBrowseTarget(peer ? { id: peer.instance_id, name: peer.instance_name } : null);
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
        aria-label={`Switch Domios host, current host ${machineLabel(label, identity)}`}
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => (open ? closeMenu() : setOpen(true))}
        className={`ct-bar-pill ct-machine-switcher ${compact ? "min-w-0 max-w-[126px] sm:max-w-none" : ""}`}
        title="Switch Domios host"
      >
        <MachineGlyph {...identity} />
        <span className="max-w-[104px] truncate sm:max-w-none">{label}</span>
        <ChevronDown size={12} className="shrink-0 opacity-60" aria-hidden="true" />
      </button>
      <AnchoredPopover open={open} anchor={rootRef} placement={compact ? "bottom-end" : "bottom-start"} onClose={dismissMenu}>
        <div
          ref={menuRef}
          data-testid="peer-picker-menu"
          role="menu"
          aria-label="Domios hosts"
          onKeyDown={onMenuKeyDown}
          className="ct-menu ct-peer-menu w-[320px]"
        >
          <button
            type="button"
            role="menuitem"
            data-testid="peer-picker-self"
            aria-label={machineLabel(localName, identities.local_machine_identity, "online")}
            title={`${localName} · ${selfHost().replace(/^https?:\/\//, "")}`}
            aria-current={onSelf ? "true" : undefined}
            onClick={() => switchTo(null)}
            className="ct-menu-row ct-machine-menu-row"
          >
            <span className="ct-menu-row__icon">
              <MachineGlyph {...identities.local_machine_identity} size="mark-15" />
            </span>
            <span className="min-w-0 flex-1 truncate">
              {localName}
            </span>
            <HostStatus status="online" />
            {onSelf && <Check aria-hidden size={14} className="ct-menu-row__check shrink-0" />}
          </button>
          {peers.map((p) => {
            const active = target?.id === p.instance_id;
            const badge = formatLastActiveBadge(p.last_active_ms);
            const compatible = p.relay_protocol_version === RELAY_PROTOCOL_VERSION;
            const available = relayPeerUsable(p);
            const status = p.online && !compatible ? "update required"
              : badge && badge !== "active now" ? badge : p.online ? "online" : "offline";
            const identity = identities.machine_identities?.[p.instance_id];
            const endpoint = p.addresses?.[0]?.ip;
            return (
              <button
                type="button"
                role="menuitem"
                key={p.instance_id}
                data-testid="peer-picker-item"
                aria-current={active ? "true" : undefined}
                aria-label={machineLabel(p.instance_name, identity, status)}
                data-peer-id={p.instance_id}
                data-peer-name={p.instance_name}
                data-peer-online={String(!!p.online)}
                disabled={!available}
                onClick={() => switchTo(p)}
                className="ct-menu-row ct-machine-menu-row min-h-[44px] sm:min-h-0"
                title={[p.instance_name, status, endpoint && `${endpoint}:${p.api_port}`].filter(Boolean).join(" · ")}
              >
                <span className="ct-menu-row__icon">
                  <MachineGlyph {...identity} size="mark-15" />
                </span>
                <span className="min-w-0 flex-1 truncate">{p.instance_name}</span>
                <HostStatus status={status} />
                {active && <Check aria-hidden size={14} className="ct-menu-row__check shrink-0" />}
              </button>
            );
          })}
          {peers.length === 0 && (
            <div className="px-2 py-1.5 text-[10.5px] text-[var(--ct-muted)]">
              No other machines yet.
            </div>
          )}
        </div>
      </AnchoredPopover>
    </div>
  );
}

function HostStatus({ status }: { status: string }) {
  const tone = status === "online" ? "var(--ct-green)" : status === "update required" ? "var(--ct-yellow)" : "var(--ct-muted)";
  return (
    <span className="flex shrink-0 items-center gap-1.5 text-[length:var(--ct-font-size-meta)] text-[var(--ct-muted-fg)]">
      <span aria-hidden="true" className="size-2 rounded-full" style={{ backgroundColor: tone }} />
      {status}
    </span>
  );
}
