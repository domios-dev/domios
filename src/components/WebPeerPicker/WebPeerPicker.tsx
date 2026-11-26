import { useCallback, useContext, useEffect, useId, useMemo, useRef, useState, useSyncExternalStore, type CSSProperties, type KeyboardEvent } from "react";
import { flushSync } from "react-dom";
import { ArrowRight, Check, ChevronDown, Network, Plus } from "lucide-react";
import type { MeshPeersResponse } from "../../../gen/MeshPeersResponse";
import type { PairedPeer } from "../../../gen/PairedPeer";
import { listen } from "../../lib/bridge";
import { effectiveWallpaper, wallpaperRenderStyle } from "../../lib/chatWallpaper";
import type { MachineAppearance } from "../../lib/chatWallpaperClient";
import { deriveColor } from "../../lib/colors";
import { hasLocalInstanceName, localInstanceLabel, onMeshPeersLoaded, rememberLocalInstanceName } from "../../lib/localInstanceName";
import { cleanMachineName, desaturateMachineColor, machineGlyphColor, machineLabel, machineTileColors, type MachineIdentity, type MeshIdentityProjection } from "../../lib/machineIdentity";
import { rankPeersByLastActive } from "../../lib/peerRanking";
import { tabActivity } from "../../lib/tabActivity";
import { isLightColor } from "../../lib/themeCss";
import { RELAY_PROTOCOL_VERSION } from "../../lib/types";
import { useMachineViewerTheme, useWallpaperAsset } from "../../lib/useMachineAppearance";
import { useMachineChromeAppearance } from "../../lib/useMachineChromeAppearance";
import { useAccountMachineNames } from "../../lib/useMeshMachineIdentities";
import { localJson } from "../../lib/webApi";
import { isMeshDisabledError } from "../../lib/remoteChatState";
import { useWorkspaceBrowseTarget } from "../../lib/workspaceBrowseTarget";
import { MuxStateStoreContext, usePairedWorkspaceInventory } from "../../store/multiplexer/context";
import { setSidebarMachineFilter, useSidebarMachineFilter } from "../Sidebar/sidebarMachineFilter";
import { ActivityDots } from "../shared/ActivityDots";
import { AnchoredPopover } from "../shared/AnchoredPopover";
import { MachineGlyph } from "../shared/MachineGlyph";
import { MachineThisBadge } from "../shared/MachineRow";
import { menuKeyAction } from "./menuKeyboard";
import "./machineSwitcher.css";

const neutralTone = "var(--ct-muted-fg)";

function useMachineDoodle(appearance: MachineAppearance | undefined, peerId: string | undefined, online: boolean) {
  const theme = appearance?.theme;
  const wallpaper = appearance?.wallpaper;
  const asset = useWallpaperAsset(peerId, online && wallpaper?.background.pattern === "svg" ? wallpaper.asset : undefined);
  return useMemo(() => {
    if (!theme) return;
    const setting = effectiveWallpaper(theme, wallpaper ?? undefined);
    const { pattern, opacity } = setting.background;
    if (pattern === "none" || pattern === "dots" || pattern === "image" || opacity === 0 || (setting.asset && !asset)) return;
    const mask = wallpaperRenderStyle(theme, setting, isLightColor(theme.background), asset?.url)["--ct-wallpaper-mask"];
    if (mask !== "none") return { mask, custom: pattern === "svg" };
  }, [theme, wallpaper, asset]);
}

function MachineDoodle({ doodle }: { doodle: ReturnType<typeof useMachineDoodle> }) {
  if (!doodle) return null;
  return <span aria-hidden="true" className="ct-machine-doodle" data-custom={doodle.custom || undefined}
    style={{ "--ct-machine-doodle-mask": doodle.mask } as CSSProperties} />;
}

function MachineDot({ peerId, online }: { peerId?: string; online: boolean }) {
  const appearance = useMachineChromeAppearance(peerId, online);
  const accent = appearance ? desaturateMachineColor(appearance.theme.blue, online) : neutralTone;
  return <i data-offline={!online || undefined} style={{ backgroundColor: accent }} />;
}

const offlineClock = new Intl.DateTimeFormat(undefined, { hour: "2-digit", minute: "2-digit" });

function peerStatus(peer: PairedPeer, availability?: string) {
  if (availability === "reconnecting") return "reconnecting";
  if (availability === "unreachable") return "unreachable";
  if (peer.online) return peer.relay_protocol_version === RELAY_PROTOCOL_VERSION ? "online" : "update required";
  const seen = peer.last_seen ? Date.parse(peer.last_seen) : NaN;
  return Number.isFinite(seen) ? `offline since ${offlineClock.format(seen)}` : "offline";
}

const noWorking = () => 0;
const noSubscription = () => () => {};

export function WebPeerPicker({ compact = false }: { compact?: boolean }) {
  const [peers, setPeers] = useState<PairedPeer[]>([]);
  const [localName, setLocalName] = useState(localInstanceLabel);
  const [open, setOpen] = useState(false);
  const target = useWorkspaceBrowseTarget();
  const filter = useSidebarMachineFilter();
  const accountNames = useAccountMachineNames();
  const { browse, generals } = usePairedWorkspaceInventory();
  const mux = useContext(MuxStateStoreContext);
  const localWorking = useSyncExternalStore(open && mux ? mux.subscribe : noSubscription, open && mux ? () => {
    const { tabs, tabStatuses } = mux.getState();
    return Object.values(tabs).filter(tab => !tab.remote_peer_id && tab.agent_type && !tab.frozen
      && tabActivity(tab, tabStatuses[tab.id]).active).length;
  } : noWorking);
  const [identities, setIdentities] = useState<MeshIdentityProjection>({});
  const menuId = useId();
  const menuRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);

  const refresh = useCallback(async () => {
    try {
      const res = await localJson<MeshPeersResponse>("GET", "/api/mesh/peers");
      const paired = res.paired ?? [];
      setLocalName(rememberLocalInstanceName(res.local_instance_name));
      setPeers(rankPeersByLastActive(paired));
      setIdentities({ local_machine_identity: res.local_machine_identity, machine_identities: res.machine_identities });
    } catch (error) {
      if (isMeshDisabledError(error)) {
        setPeers(current => current.some(peer => peer.online)
          ? current.map(peer => (peer.online ? { ...peer, online: false } : peer))
          : current);
      }
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

  const close = () => {
    setOpen(false);
    triggerRef.current?.focus({ preventScroll: true });
  };

  function openMesh(meshTab: "inventory" | "network") {
    const previousFocus = document.activeElement;
    flushSync(() => {
      setOpen(false);
      window.dispatchEvent(new CustomEvent("codeterm:open-panel", { detail: { panel: "mesh", meshTab } }));
    });
    if (document.activeElement === previousFocus || document.activeElement === document.body) {
      triggerRef.current?.focus({ preventScroll: true });
    }
  }

  useEffect(() => {
    if (!open) return;
    const selected = menuRef.current?.querySelector<HTMLElement>('[aria-selected="true"]');
    (selected ?? menuRef.current?.querySelector<HTMLElement>('[role="option"]'))?.focus();
  }, [open]);

  const onOptionKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Tab") {
      close();
      return;
    }
    const items = Array.from(event.currentTarget.querySelectorAll("button"));
    const focused = items.indexOf(document.activeElement as HTMLButtonElement);
    if (event.key === "Enter" && focused !== -1) {
      event.preventDefault();
      items[focused].click();
      return;
    }
    const action = menuKeyAction(event.key, focused, items.length);
    if (!action) return;
    event.preventDefault();
    if (action.kind === "close") close();
    else items[action.index]?.focus();
  };

  const peerName = (peer: PairedPeer) => accountNames.peers.get(peer.instance_id)
    ?? cleanMachineName(peer.instance_name, peer.instance_id) ?? "Machine";
  const selectedPeer = peers.find(peer => peer.instance_id === target?.id);
  const selectedGeneral = target ? generals.find(general => general.peerId === target.id) : undefined;
  const selectedAvailability = target
    ? (browse?.target.id === target.id ? browse.availability : selectedGeneral?.availability)
    : undefined;
  const local = { name: accountNames.local ?? localName, identity: identities.local_machine_identity };
  const singleMachine = peers.length === 0;
  const all = !singleMachine && filter === "all";
  const localSelected = filter === "local" || (singleMachine && !target);
  const selectedReconnecting = !all && Boolean(target) && (
    selectedAvailability === "reconnecting"
    || (browse?.target.id === target?.id && browse.synced === false)
  );
  const label = all ? "All machines" : target
    ? selectedPeer ? peerName(selectedPeer) : cleanMachineName(target.name, target.id) ?? "Machine"
    : local.name;
  const identity = target ? identities.machine_identities?.[target.id] : local.identity;
  const viewer = useMachineViewerTheme();
  const selectedOnline = !target || (selectedPeer?.online === true && selectedAvailability !== "unreachable");
  const selectedAppearance = useMachineChromeAppearance(target?.id, selectedOnline, !all);
  const selectedDoodle = useMachineDoodle(selectedAppearance, target?.id, selectedOnline);
  const selectedTone = !all && selectedAppearance ? machineGlyphColor(
    desaturateMachineColor(selectedAppearance.theme.blue, selectedOnline), viewer.background, viewer.foreground, 0.15,
  ) : undefined;
  const onlineCount = 1 + peers.filter(peer => peer.online).length;
  const machineCount = `${peers.length + 1} machines`;

  function select(selection: Parameters<typeof setSidebarMachineFilter>[0], option: HTMLButtonElement, animate = true) {
    const bounds = option.getBoundingClientRect();
    const origin = bounds.width && bounds.height ? bounds : triggerRef.current?.getBoundingClientRect();
    setSidebarMachineFilter(selection, animate && origin
      ? { x: origin.left + origin.width / 2, y: origin.top + origin.height / 2 }
      : undefined);
    close();
  }

  return (
    <div className="relative" data-testid="peer-picker">
      <button
        ref={triggerRef}
        type="button"
        data-testid="peer-picker-button"
        aria-label={selectedReconnecting ? `Machine: ${label} (reconnecting)` : `Machine: ${label}`}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-controls={open ? menuId : undefined}
        data-offline={!all && !selectedOnline || undefined}
        data-reconnecting={selectedReconnecting || undefined}
        style={{ "--ct-switcher-tone": selectedTone } as CSSProperties}
        onClick={() => open ? close() : setOpen(true)}
        onKeyDown={event => {
          if (!open && (event.key === "ArrowDown" || event.key === "ArrowUp")) {
            event.preventDefault();
            setOpen(true);
          }
        }}
        className={`ct-bar-pill ct-machine-switcher ${compact ? "min-w-0 max-w-[min(10rem,40vw)] sm:max-w-none" : ""}`}
      >
        {!all && <MachineDoodle doodle={selectedDoodle} />}
        {all ? <Network size={12} className="shrink-0" aria-hidden="true" /> : <MachineGlyph {...identity} tone={selectedTone ?? neutralTone} />}
        <span className="ct-machine-switcher__label min-w-0 truncate">{label}</span>
        {selectedReconnecting && (
          <span role="status" data-testid="peer-picker-reconnecting" className="inline-flex items-center gap-1 text-[10px] opacity-80 shrink-0">
            <ActivityDots size="micro" active />
            <span className="hidden min-[400px]:inline">Reconnecting</span>
          </span>
        )}
        {localSelected && <MachineThisBadge className="ct-machine-switcher__this hidden min-[400px]:inline-flex" />}
        <ChevronDown size={12} className="ct-machine-switcher__chevron shrink-0 opacity-60" aria-hidden="true" />
      </button>
      <AnchoredPopover open={open} anchor={triggerRef} placement={compact ? "bottom-end" : "bottom-start"} onClose={() => setOpen(false)}>
        <div ref={menuRef}
          onKeyDown={onOptionKeyDown} onBlur={event => {
            if (event.relatedTarget && !event.currentTarget.contains(event.relatedTarget) && event.relatedTarget !== triggerRef.current) setOpen(false);
          }}
          className="ct-menu ct-menu--opaque ct-machine-list"
        >
          <div id={menuId} data-testid="peer-picker-menu" role="listbox" aria-label="Machines">
            {!singleMachine && <button type="button" role="option" tabIndex={-1} data-testid="peer-picker-all" aria-selected={all}
              aria-label={`All machines, ${machineCount}, ${onlineCount} online`}
              onClick={event => select("all", event.currentTarget)} className="ct-machine-option ct-machine-option--all">
              <span className="ct-machine-constellation" aria-hidden="true"
                style={{ gridTemplateColumns: `repeat(${Math.ceil(Math.sqrt(peers.length + 1))}, minmax(0, 1fr))` }}>
                <MachineDot online />
                {peers.map(peer => <MachineDot key={peer.instance_id} peerId={peer.instance_id} online={peer.online} />)}
              </span>
              <span className="ct-machine-option__copy">
                <span>All machines</span>
                <span className="ct-machine-option__status">{machineCount} · {onlineCount} online</span>
              </span>
              {all && <Check size={16} className="ct-machine-option__check" aria-hidden="true" />}
            </button>}
            <MachineOption {...local} local status="online" working={localWorking} online selected={localSelected}
              viewer={viewer}
              onSelect={option => select("local", option)} />
            {peers.map(peer => {
              const name = peerName(peer);
              const general = generals.find(candidate => candidate.peerId === peer.instance_id);
              const status = peerStatus(peer, general?.availability);
              const working = browse?.target.id === peer.instance_id && browse.availability === "online"
                ? observedWorkingCount(general?.sessions ?? [], browse.workspaces.flatMap(workspace => workspace.tabs))
                : 0;
              return <MachineOption key={peer.instance_id} name={name} identity={identities.machine_identities?.[peer.instance_id]}
                peerId={peer.instance_id} status={status} working={working} online={peer.online} selected={target?.id === peer.instance_id}
                viewer={viewer}
                onSelect={option => select({ id: peer.instance_id, name }, option, status === "online")} />;
            })}
          </div>
          <div role="group" aria-label="Mesh actions" className="ct-machine-footer" data-invitation={singleMachine || undefined}>
            {singleMachine && <>
              <p className="ct-machine-footer__title">Work across your computers</p>
              <p className="ct-machine-footer__description">Install Domios on another computer and pair it to see its agents here.</p>
            </>}
            <div className="ct-machine-footer__actions">
              <button type="button" tabIndex={-1} className="ct-machine-footer__add" onClick={() => openMesh("network")}>
                <span className="ct-machine-footer__plus" aria-hidden="true"><Plus size={14} /></span>
                Add machine
              </button>
              <button type="button" tabIndex={-1} className="ct-machine-footer__mesh" onClick={() => openMesh("inventory")}>
                {!singleMachine && <Network size={13} aria-hidden="true" />}
                Open Mesh
                <ArrowRight size={12} aria-hidden="true" />
              </button>
            </div>
          </div>
        </div>
      </AnchoredPopover>
    </div>
  );
}

type ObservedTab = import("../../store/multiplexer/pairedWorkspaceProjection").ObservedPeerTab;

function observedWorkingCount(general: ObservedTab[], desk: ObservedTab[]) {
  const seen = new Set<string>();
  for (const { viewerId, tab, status } of [...general, ...desk]) {
    if (tab.agent_type && !tab.frozen && tabActivity(tab, status).active) seen.add(viewerId);
  }
  return seen.size;
}

function MachineOption({ name, identity, peerId, status, working, local, online, selected, viewer, onSelect }: {
  name: string;
  identity?: MachineIdentity;
  peerId?: string;
  status: string;
  working: number;
  local?: boolean;
  online: boolean;
  selected: boolean;
  viewer: MachineAppearance["theme"];
  onSelect: (option: HTMLButtonElement) => void;
}) {
  const busy = status === "online" && working > 0;
  const appearance = useMachineChromeAppearance(peerId, online);
  const doodle = useMachineDoodle(appearance, peerId, online);
  const colors = appearance ? machineTileColors(
    desaturateMachineColor(appearance.theme.blue, online), deriveColor(viewer.background, 10), viewer.foreground, selected,
  ) : { tone: neutralTone, glyph: neutralTone };
  const state = busy ? `${working} working` : status;
  return <button type="button" role="option" tabIndex={-1} aria-selected={selected}
    aria-label={machineLabel(name, identity, local ? `this machine, ${state}` : state)}
    data-testid={local ? "peer-picker-self" : "peer-picker-item"} data-peer-id={peerId} data-peer-name={peerId ? name : undefined}
    data-peer-online={peerId ? String(online) : undefined} data-offline={!online || undefined}
    className="ct-machine-option" style={{ "--ct-switcher-tone": colors.tone } as CSSProperties}
    onClick={event => onSelect(event.currentTarget)}>
    <span className="ct-machine-option__tile">
      <MachineDoodle doodle={doodle} />
      <MachineGlyph {...identity} size="mark-15" tone={colors.glyph} />
    </span>
    <span className="ct-machine-option__copy">
      <span className="ct-machine-option__name"><span className="truncate">{name}</span>{local && <MachineThisBadge />}</span>
      <span className="ct-machine-option__status">
        {busy ? <ActivityDots size="small" /> : <span aria-hidden="true" className="ct-machine-option__dot" data-online={online && status === "online" || undefined} />}
        <span className="truncate">{state}</span>
      </span>
    </span>
    {selected && <Check size={16} className="ct-machine-option__check" aria-hidden="true" />}
  </button>;
}
