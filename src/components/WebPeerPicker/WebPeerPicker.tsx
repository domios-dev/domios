import { useCallback, useEffect, useRef, useState, type KeyboardEvent as ReactKeyboardEvent } from "react";
import { flushSync } from "react-dom";
import { createPortal } from "react-dom";
import { listen } from "../../lib/bridge";
import { resetWebEvents } from "../../lib/webEvents";
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
import { clampFloatingPosition, type FloatingPosition } from "../shared/floatingPosition";
import { menuKeyAction } from "./menuKeyboard";
import { RELAY_PROTOCOL_VERSION } from "../../lib/types";

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
  const [localName, setLocalName] = useState("CodeTerm");
  const [open, setOpen] = useState(false);
  const [displayTarget, setDisplayTarget] = useState(relayTarget);
  const [switchingTo, setSwitchingTo] = useState<string | null>(null);
  const [switchFailed, setSwitchFailed] = useState(false);
  const rootRef = useRef<HTMLDivElement | null>(null);
  const menuRef = useRef<HTMLDivElement | null>(null);
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  const [menuPosition, setMenuPosition] = useState<FloatingPosition | null>(null);

  const refresh = useCallback(async () => {
    try {
      const res = await localJson<MeshPeers>("GET", "/api/mesh/peers");
      const paired = res.paired ?? [];
      setLocalName(res.local_instance_name);
      setPeers(rankPeersByLastActive(paired));
    } catch {
      // Peer list is a convenience — keep the last known list on errors.
    }
  }, []);

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;
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
      unlisten?.();
    };
  }, [refresh]);

  useEffect(() => {
    if (!open) return;
    const close = (e: MouseEvent) => {
      if (!rootRef.current?.contains(e.target as Node) && !menuRef.current?.contains(e.target as Node)) {
        setOpen(false);
      }
    };
    window.addEventListener("mousedown", close);
    return () => window.removeEventListener("mousedown", close);
  }, [open]);

  const placeMenu = useCallback(() => {
    const rect = rootRef.current?.getBoundingClientRect();
    if (!rect) return;
    const width = Math.min(320, Math.max(220, window.innerWidth - 8));
    const height = Math.min((peers.length + 1) * 44 + 8, window.innerHeight - 8);
    const x = compact ? rect.right - width : rect.left;
    setMenuPosition(clampFloatingPosition(x, rect.bottom + 4, width, height, window.innerWidth, window.innerHeight));
  }, [compact, peers.length]);

  useEffect(() => {
    if (!open) {
      setMenuPosition(null);
      return;
    }
    placeMenu();
    window.addEventListener("resize", placeMenu);
    window.addEventListener("scroll", placeMenu, true);
    return () => {
      window.removeEventListener("resize", placeMenu);
      window.removeEventListener("scroll", placeMenu, true);
    };
  }, [open, placeMenu]);

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

  // An explicitly opened menu takes focus so arrows and Escape reach it.
  useEffect(() => {
    if (!open || !menuPosition) return;
    menuItems()[0]?.focus();
  }, [open, menuPosition, menuItems]);

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
        aria-label={`Switch CodeTerm host, current host ${label}`}
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => (open ? closeMenu() : setOpen(true))}
        className={`flex min-h-[44px] items-center gap-1.5 rounded text-xs text-[var(--ct-fg)] bg-[var(--ct-hover)] hover:bg-[var(--ct-active)] border border-[var(--ct-border)] sm:min-h-0 sm:h-7 ${compact ? "min-w-0 max-w-[126px] px-2" : "px-2.5"}`}
        title="Switch CodeTerm host"
      >
        <span className={`w-2 h-2 shrink-0 rounded-full ${switchingTo ? "animate-pulse bg-[var(--ct-muted,#8b949e)]" : switchFailed ? "bg-[var(--ct-red,#f85149)]" : "bg-[var(--ct-green,#3fb950)]"}`} />
        <span className="max-w-[104px] truncate sm:max-w-[120px]">{label}</span>
        <span className="opacity-60 text-[9px]">▾</span>
      </button>
      {open && menuPosition && createPortal(
        <div
          ref={menuRef}
          data-testid="peer-picker-menu"
          role="menu"
          aria-label="CodeTerm hosts"
          onKeyDown={onMenuKeyDown}
          className="fixed z-50 w-[min(320px,calc(100vw-0.5rem))] overflow-y-auto rounded border border-[var(--ct-border)] bg-[var(--ct-titlebar-bg)] shadow-lg py-1"
          style={{ top: menuPosition.top, left: menuPosition.left, maxHeight: menuPosition.maxHeight }}
        >
          <button
            type="button"
            role="menuitem"
            data-testid="peer-picker-self"
            aria-current={onSelf ? "true" : undefined}
            onClick={() => switchTo(null)}
            className={`w-full text-left px-3 min-h-[44px] flex items-center gap-2 text-xs hover:bg-[var(--ct-hover)] ${onSelf ? "text-[var(--ct-accent,#58a6ff)]" : "text-[var(--ct-fg)]"}`}
          >
            <span className="w-2 h-2 rounded-full bg-[var(--ct-green,#3fb950)]" />
            <span className="flex-1 truncate">
              {localName} ({selfHost().replace(/^https?:\/\//, "")})
            </span>
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
                data-peer-name={p.instance_name}
                data-peer-online={String(!!p.online)}
                disabled={!available}
                onClick={() => switchTo(p)}
                className={`w-full text-left px-3 min-h-[44px] flex items-center gap-2 text-xs hover:bg-[var(--ct-hover)] disabled:opacity-40 ${active ? "text-[var(--ct-accent,#58a6ff)]" : "text-[var(--ct-fg)]"}`}
                title={!p.online ? `${p.instance_name} is offline` : compatible ? p.instance_name : `Update CodeTerm on ${p.instance_name}`}
              >
                <span
                  className={`w-2 h-2 rounded-full shrink-0 ${p.online ? "bg-[var(--ct-green,#3fb950)]" : "bg-[var(--ct-muted,#8b949e)]"}`}
                />
                <span className="flex-1 truncate">{p.instance_name}</span>
                {badge && (
                  <span className="text-[9px] opacity-60 shrink-0">{badge}</span>
                )}
                {!p.online && !badge && (
                  <span className="text-[9px] opacity-60 shrink-0">offline</span>
                )}
                {p.online && !compatible && (
                  <span className="text-[9px] opacity-60 shrink-0">update required</span>
                )}
              </button>
            );
          })}
          {peers.length === 0 && (
            <div className="px-3 py-2 text-[10px] text-[var(--ct-muted,#8b949e)]">
              no paired peers
            </div>
          )}
        </div>,
        document.body,
      )}
    </div>
  );
}
