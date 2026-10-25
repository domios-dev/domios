import { useCallback, useEffect, useRef, useState, type KeyboardEvent as ReactKeyboardEvent } from "react";
import { Check, ChevronDown } from "lucide-react";
import { flushSync } from "react-dom";
import { listen } from "../../lib/bridge";
import { resetWebEvents } from "../../lib/webEvents";
import { resetOutputStream } from "../../lib/outputStream";
import { formatLastActiveBadge, rankPeersByLastActive } from "../../lib/peerRanking";
import {
  clearRelayTarget,
  localJson,
  relayPeerUsable,
  relayTarget,
  setRelayTarget,
  selfHost,
  invalidateWebRequestContext,
} from "../../lib/webApi";
import { AnchoredPopover } from "../shared/AnchoredPopover";
import { menuKeyAction } from "./menuKeyboard";
import { RELAY_PROTOCOL_VERSION } from "../../lib/types";
import { hasLocalInstanceName, localInstanceLabel, onMeshPeersLoaded, rememberLocalInstanceName } from "../../lib/localInstanceName";

let nextPeerSwitchId = 0;

interface PeerAddress {
  ip: string;
  is_tailscale: boolean;
}
interface PairedPeer {
  instance_id: string;
  instance_name: string;
  addresses: PeerAddress[];
  api_port: number;
  online: boolean;
  relay_protocol_version: number;
  last_active_ms?: number | null;
}
interface MeshPeers {
  paired: PairedPeer[];
  local_instance_name: string;
}

/**
 * Web-mirror peer switcher (top-left). Lists the LOCAL daemon's paired mesh
 * peers; selecting one sets a relay target so every request is proxied
 * through the local daemon's mesh relay (the browser stays pointed at its own
 * daemon — no re-point, no peer-scoped token). Selecting the gateway machine
 * clears the target. In-place switch reattaches events and refreshes mux state.
 */
export function WebPeerPicker({ compact = false }: { compact?: boolean }) {
  const [peers, setPeers] = useState<PairedPeer[]>([]);
  const [localName, setLocalName] = useState(localInstanceLabel);
  const [open, setOpen] = useState(false);
  const [displayTarget, setDisplayTarget] = useState(relayTarget);
  const [switchingTo, setSwitchingTo] = useState<string | null>(null);
  const [switchFailed, setSwitchFailed] = useState(false);
  const rootRef = useRef<HTMLDivElement | null>(null);
  const menuRef = useRef<HTMLDivElement | null>(null);
  const triggerRef = useRef<HTMLButtonElement | null>(null);

  const refresh = useCallback(async () => {
    try {
      const res = await localJson<MeshPeers>("GET", "/api/mesh/peers");
      const paired = res.paired ?? [];
      setLocalName(rememberLocalInstanceName(res.local_instance_name));
      setPeers(rankPeersByLastActive(paired));
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
    return () => {
      active = false;
      stopPeersLoaded();
      unlisten?.();
    };
  }, [refresh]);

  useEffect(() => {
    const ready = (event: Event) => {
      const detail = (event as CustomEvent<{ switchId: number; success: boolean }>).detail;
      if (detail.switchId !== nextPeerSwitchId) return;
      setDisplayTarget(relayTarget());
      setSwitchingTo(null);
      setSwitchFailed(!detail.success);
    };
    window.addEventListener("codeterm-peer-switch-ready", ready);
    return () => window.removeEventListener("codeterm-peer-switch-ready", ready);
  }, []);

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

  const target = displayTarget;
  const onSelf = !target;
  const label = switchingTo ? `Connecting to ${switchingTo}` : onSelf ? localName : target.name;

  const switchTo = useCallback(
    (peer: PairedPeer | null) => {
      const switchId = ++nextPeerSwitchId;
      const nextName = peer?.instance_name ?? localName;
      setSwitchingTo(nextName);
      setSwitchFailed(false);
      invalidateWebRequestContext();
      flushSync(() => {
        window.dispatchEvent(new CustomEvent("codeterm-peer-switch-reset", { detail: { switchId } }));
      });
      if (peer) {
        setRelayTarget(peer.instance_id, peer.instance_name);
      } else {
        clearRelayTarget();
      }
      invalidateWebRequestContext();
      closeMenu();
      resetOutputStream();
      resetWebEvents();
      window.dispatchEvent(new CustomEvent("codeterm-peer-switch-rebind", { detail: { switchId } }));
    },
    [closeMenu, localName],
  );

  return (
    <div ref={rootRef} className="relative" data-testid="peer-picker">
      <button
        ref={triggerRef}
        type="button"
        data-testid="peer-picker-button"
        aria-label={`Switch Domios host, current host ${label}`}
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => (open ? closeMenu() : setOpen(true))}
        className={`flex min-h-[44px] items-center gap-1.5 rounded text-xs text-[var(--ct-fg)] bg-[var(--ct-hover)] hover:bg-[var(--ct-active)] border border-[var(--ct-border)] sm:min-h-0 sm:h-7 ${compact ? "min-w-0 max-w-[126px] px-2 sm:max-w-none" : "px-2.5"}`}
        title="Switch Domios host"
      >
        <span className={`w-2 h-2 shrink-0 rounded-full ${switchingTo ? "animate-pulse bg-[var(--ct-muted,#8b949e)]" : switchFailed ? "bg-[var(--ct-red,#f85149)]" : "bg-[var(--ct-green,#3fb950)]"}`} />
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
            aria-current={onSelf ? "true" : undefined}
            onClick={() => switchTo(null)}
            className="ct-menu-row"
          >
            <span className="ct-menu-row__icon">
              <span className="w-2 h-2 rounded-full bg-[var(--ct-green)]" />
            </span>
            <span className="min-w-0 flex-1 truncate">
              {localName} ({selfHost().replace(/^https?:\/\//, "")})
            </span>
            {onSelf && <Check aria-hidden size={14} className="ct-menu-row__check shrink-0" />}
          </button>
          {peers.map((p) => {
            const active = target?.id === p.instance_id;
            const badge = formatLastActiveBadge(p.last_active_ms);
            const compatible = p.relay_protocol_version === RELAY_PROTOCOL_VERSION;
            const available = relayPeerUsable(p);
            return (
              <button
                type="button"
                role="menuitem"
                key={p.instance_id}
                data-testid="peer-picker-item"
                aria-current={active ? "true" : undefined}
                data-peer-id={p.instance_id}
                data-peer-name={p.instance_name}
                data-peer-online={String(!!p.online)}
                disabled={!available}
                onClick={() => switchTo(p)}
                className="ct-menu-row"
                title={!p.online ? `${p.instance_name} is offline` : compatible ? p.instance_name : `Update Domios on ${p.instance_name}`}
              >
                <span className="ct-menu-row__icon">
                  <span className={`w-2 h-2 rounded-full ${p.online ? "bg-[var(--ct-green)]" : "bg-[var(--ct-muted)]"}`} />
                </span>
                <span className="min-w-0 flex-1 truncate">{p.instance_name}</span>
                {badge && (
                  <span className="shrink-0 text-[10px] text-[var(--ct-muted)]">{badge}</span>
                )}
                {!p.online && !badge && (
                  <span className="shrink-0 text-[10px] text-[var(--ct-muted)]">offline</span>
                )}
                {p.online && !compatible && (
                  <span className="shrink-0 text-[10px] text-[var(--ct-muted)]">update required</span>
                )}
                {active && <Check aria-hidden size={14} className="ct-menu-row__check shrink-0" />}
              </button>
            );
          })}
          {peers.length === 0 && (
            <div className="px-2 py-1.5 text-[10.5px] text-[var(--ct-muted)]">
              no paired peers
            </div>
          )}
        </div>
      </AnchoredPopover>
    </div>
  );
}
