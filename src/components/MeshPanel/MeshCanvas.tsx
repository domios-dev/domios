import { LOCAL_MACHINE_ID } from "../../lib/machinePlacement";
import { useCanvasStage, CANVAS_ZOOM_STEP } from "@codeterm/chat-ui/useCanvasStage";
import { LocalMachineFooter, MachineFooter, ACCESS_LABELS } from "./MachineFooter";
import { REMOVED_COPY } from "./accountMachines";
import { peerInventoryModel } from "./peerInventoryModel";
import type { Service } from "./types";
import { memo, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { Globe, Network, Plus } from "lucide-react";
import { PeerTerminal } from "./PeerTerminal";
import { CloudMachineCard } from "./CloudMachineCard";
import { CloudMachineSlot } from "./CloudMachineSlot";
import type { PendingCloudCreate } from "./useCloudMachines";
import { cloudMachineCardView, type CloudFailurePresentation } from "../../lib/cloudMachines/stateModel";
import type { CloudReceipt, CloudUsagePayload } from "../../lib/cloudMachines/types";
import { lastSeenMs, type AccountCanvas, type AccountCanvasCard, type AccountMachineNode } from "./accountCanvas";
import { AccountMachineCard, AccountNotes, AccountStateCard, MachineReauthentication, PeerAccountFooterView, type MachineNodeActions } from "./AccountCanvasNodes";
import { formatAge } from "../../lib/timeLabels";
import { meshEdgeModel } from "./meshEdgeModel";
import type { PeerStatus } from "./useMeshData";
import { CanvasToolbar } from "@codeterm/chat-ui/CanvasToolbar";
import { formatLayerTransform, AUTO_FIT_MAX_SCALE } from "@codeterm/canvas-camera";
import { webLinkWithTokenAndRelay } from "../../lib/webAccess";
import { RELAY_PROTOCOL_VERSION } from "../../lib/types";
import { webGraphLayout, WEB_CARD_SCALE, MACHINE_CARD_SCALE, MACHINE_CARD_WIDTH } from "./webGraphLayout";
import { webGatewayGraph } from "./meshProjection";
import { TailscaleFooter, TailscaleMachineHint, type CanvasTailscale } from "./TailscaleNodes";
import { differentNetwork } from "./networkTransport";
import { offersTailscaleSetup, suggestsRemoteTailscale } from "./tailscaleSetup";
import { Button } from "../shared/Button";
import { DuplicateMachineNameHint } from "./DuplicateMachineNameHint";
import { MeshOverview } from "./MeshOverview";
import { meshOverviewState } from "./meshOverviewModel";
import { MachineLimitPaywall } from "./MachineLimitPaywall";
import type { MyMachines } from "./useMyMachines";
import { offersInventoryList } from "./meshViewportModel";
import { MeshMachineList } from "./MeshMachineList";
import { MeshListSuggestionBanner } from "./MeshListSuggestionBanner";
import { MeshViewToggle } from "./MeshViewToggle";
import { useMeshViewMode } from "./useMeshViewMode";

export interface RecentPush {
  source_id: string;
  target: string;
  at: number;
}

interface PeerLayout {
  id: string;
  name: string;
  ip: string;
  x: number;
  y: number;
  height: number;
  isThisMachine?: boolean;
  isPaired?: boolean;
  pending?: boolean;
  accessLevel?: string;
  account?: { kind: "node"; node: AccountMachineNode } | { kind: "card"; card: AccountCanvasCard };
  cloud?: {
    kind: "machine";
    machine: CloudReceipt;
    peer?: PeerStatus;
  } | {
    kind: "pending";
    pending: PendingCloudCreate;
  } | {
    kind: "slot";
  };
  webUrl?: string | null;
  webUnavailableReason?: string;
}

export interface CanvasCloud {
  machines: CloudReceipt[];
  usage: CloudUsagePayload | null;
  isFree: boolean;
  slotOpen: boolean;
  pendingCreate: PendingCloudCreate | null;
  selectedMachineId?: string | null;
  busyMachineId?: string | null;
  onOpenSlot: () => void;
  onCloseSlot: () => void;
  onCreateCloud: (size: string) => void;
  onPairComputer: () => void;
  onSelectMachine: (machineId: string) => void;
  onTopUpMachine: (machine: CloudReceipt) => void;
  onSleepMachine?: (machineId: string) => Promise<unknown>;
  onWakeMachine?: (machineId: string) => Promise<unknown>;
  onFailureAction: (action: CloudFailurePresentation["action"]) => void;
}

/** My machines on the canvas: account nodes, paired-card footers and the actions behind them. */
export interface CanvasAccount {
  selfName?: string;
  selfDisplayName?: string | null;
  selfSameNameOthers?: number;
  onRenameSelf?: (name: string) => Promise<boolean>;
  canvas: AccountCanvas;
  busy: string | null;
  signingIn: boolean;
  cancelSignIn?: () => void;
  reauthRequired?: boolean;
  removed?: boolean;
  onRejoin?: () => void;
  limitMachines?: MyMachines;
  moreMachines?: boolean;
  /** Epoch milliseconds. */
  now: number;
  actions: MachineNodeActions;
  onSignIn: () => void;
  onRetry: () => void;
  onRetryEnrollment?: () => void;
}

interface Props {
  webCard?: (qrSize: number) => ReactNode;
  onRefresh?: () => void;
  onWebClick?: () => void;
  webSelected?: boolean;
  webStatus?: string;
  thisMachine: { name: string; ip: string };
  peers: PeerStatus[];
  services?: Service[];
  loading?: boolean;
  error?: string | null;
  recentPushes: RecentPush[];
  onPeerClick: (peerId: string) => void;
  width: number;
  height: number;
  account?: CanvasAccount;
  cloud?: CanvasCloud;
  tailscale?: CanvasTailscale;
  meshEnabled?: boolean;
  onGoOnline?: () => void;
  onOpenPeerWebUi?: (url: string) => void;
  webUiBaseUrl?: string | null;
  webUiToken?: string | null;
  touch?: boolean;
  selectedPeerId?: string | null;
}

const MIN_SCALE = 0.001;
const MAX_SCALE = 2.5;
const FIT_PADDING = 24;
const CARD_HALF_WIDTH = MACHINE_CARD_WIDTH / 2;

export const MeshCanvas = memo(function MeshCanvas({
  webCard,
  onRefresh,
  onWebClick,
  webSelected = false,
  webStatus = "Unavailable",
  thisMachine,
  peers,
  services = [],
  loading = false,
  error,
  onPeerClick,
  width,
  height,
  account,
  cloud,
  tailscale,
  meshEnabled = true,
  onGoOnline,
  onOpenPeerWebUi,
  webUiBaseUrl = null,
  webUiToken = null,
  touch = false,
  selectedPeerId = null,
}: Props) {
  const [showInventory, setShowInventory] = useState(false);
  const narrow = offersInventoryList(width, touch);
  const iconOnlyToggle = width > 0 && width < 480;
  const zoomStep = CANVAS_ZOOM_STEP;
  const webCardRef = useRef<HTMLDivElement>(null);
  const [webHeight, setWebHeight] = useState(800);
  const [machineHeights, setMachineHeights] = useState<number[]>([]);
  const machineLayerRef = useRef<HTMLDivElement>(null);
  const cloudMachines = useMemo(
    () => (cloud?.machines ?? []).filter((machine) => machine.state !== "deleted"),
    [cloud?.machines],
  );
  const cloudPeerIds = useMemo(() => {
    const ids = new Set<string>();
    for (const machine of cloudMachines) {
      if (machine.peer_instance_id) ids.add(machine.peer_instance_id);
      if (machine.installation_id) ids.add(machine.installation_id);
      ids.add(machine.machine_id);
    }
    return ids;
  }, [cloudMachines]);
  const nonCloudPeers = useMemo(
    () => peers.filter((peer) => !cloudPeerIds.has(peer.instance_id)),
    [peers, cloudPeerIds],
  );
  const accountNodes = useMemo(
    () => (account?.canvas.nodes ?? []).filter((node) => !cloudPeerIds.has(node.key) && (!node.row.pairInstanceId || !cloudPeerIds.has(node.row.pairInstanceId))),
    [account?.canvas.nodes, cloudPeerIds],
  );
  const machineCount = account?.canvas.card?.kind === "loading"
    ? null
    : 1 + nonCloudPeers.length + cloudMachines.length + accountNodes.length;
  const viewState = useMeshViewMode(machineCount, narrow);
  const shown = viewState.viewMode === "list" ? "list" : "graph";
  useLayoutEffect(() => {
    const element = webCardRef.current;
    if (!element) return;
    if (element.offsetHeight > 0) setWebHeight(element.offsetHeight);
    const observer = new ResizeObserver(() => { if (element.offsetHeight > 0) setWebHeight(element.offsetHeight); });
    observer.observe(element);
    return () => observer.disconnect();
  }, [shown]);
  const showCloudSlot = Boolean(cloud && (cloud.pendingCreate || cloud.slotOpen));
  const accountCard = cloudMachines.length > 0 || showCloudSlot ? null : (account?.canvas.card ?? null);
  const extraCount = accountNodes.length + (accountCard ? 1 : 0) + cloudMachines.length + (showCloudSlot ? 1 : 0);
  const geometry = webGraphLayout(width, height, nonCloudPeers.length + 1 + extraCount, webHeight, machineHeights);
  const layout = useMemo<PeerLayout[]>(() => {
    const items: PeerLayout[] = [
      {
        id: "__self__",
        name: account?.selfName ?? thisMachine.name,
        ip: thisMachine.ip,
        ...geometry.machines[0],
        isThisMachine: true,
        webUrl: webUiBaseUrl
          ? webLinkWithTokenAndRelay(webUiBaseUrl, webUiToken, null)
          : null,
        webUnavailableReason: webUiBaseUrl ? undefined : "Web UI is not enabled on this machine",
      },
    ];
    nonCloudPeers.forEach((p, i) => {
      const signedOut = account?.canvas.peerFooters.get(p.instance_id)?.row.status === "signed_out";
      items.push({
        id: p.instance_id,
        name: account?.canvas.peerFooters.get(p.instance_id)?.row.name ?? p.name,
        ip: p.ip,
        ...geometry.machines[i + 1],
        // A discovered-but-not-paired (vouched) peer must never render as paired (codeterm#440).
        isPaired: !p.pending,
        pending: p.pending,
        accessLevel: p.access_level,
        webUrl: !signedOut && p.online && p.relay_protocol_version === RELAY_PROTOCOL_VERSION && webUiBaseUrl
          ? webLinkWithTokenAndRelay(webUiBaseUrl, webUiToken, {
              id: p.instance_id,
            })
          : null,
        webUnavailableReason: signedOut
          ? `${p.name} is signed out`
          : !p.online
          ? `${p.name} is offline`
          : p.relay_protocol_version !== RELAY_PROTOCOL_VERSION
            ? `Update Domios on ${p.name} to use Web UI relay`
            : "The browser gateway is unavailable",
      });
    });
    for (const machine of cloudMachines) {
      const matchedPeer = peers.find(
        (candidate) =>
          candidate.instance_id === machine.peer_instance_id ||
          candidate.instance_id === machine.installation_id ||
          candidate.instance_id === machine.machine_id,
      );
      items.push({
        id: `cloud:${machine.machine_id}`,
        name: machine.name,
        ip: matchedPeer?.ip ?? "",
        ...geometry.machines[items.length],
        cloud: { kind: "machine", machine, peer: matchedPeer },
      });
    }
    if (cloud?.pendingCreate) {
      items.push({
        id: "cloud:new-slot",
        name: "Cloud machine",
        ip: "",
        ...geometry.machines[items.length],
        cloud: { kind: "pending", pending: cloud.pendingCreate },
      });
    } else if (cloud?.slotOpen) {
      items.push({
        id: "cloud:new-slot",
        name: "New machine",
        ip: "",
        ...geometry.machines[items.length],
        cloud: { kind: "slot" },
      });
    }
    for (const node of accountNodes) {
      items.push({ id: `account:${node.key}`, name: node.name, ip: "", ...geometry.machines[items.length], account: { kind: "node", node } });
    }
    if (accountCard) {
      items.push({ id: `account-card:${accountCard.kind}`, name: "My machines", ip: "", ...geometry.machines[items.length], account: { kind: "card", card: accountCard } });
    }
    return items;
  }, [thisMachine, nonCloudPeers, peers, cloudMachines, cloud?.pendingCreate, cloud?.slotOpen, account, accountNodes, accountCard, webUiBaseUrl, webUiToken, geometry.machines]);

  useLayoutEffect(() => {
    const nodes = Array.from(machineLayerRef.current?.querySelectorAll<HTMLElement>("[data-machine-index]") ?? []);
    const measure = () => {
      const heights = nodes.map((node) => node.offsetHeight * MACHINE_CARD_SCALE);
      if (heights.some((height) => height <= 0)) return;
      setMachineHeights((current) => heights.length === current.length && heights.every((height, index) => height === current[index]) ? current : heights);
    };
    measure();
    const observer = new ResizeObserver(measure);
    nodes.forEach((node) => observer.observe(node));
    return () => observer.disconnect();
  }, [layout.length, shown]);

  const gateway = { x: width / 2, y: height / 2 };
  const gatewayGraph = webGatewayGraph(nonCloudPeers, meshEnabled);
  const stage = useCanvasStage({
    initialSize: { width, height }, measureKey: shown,
    followFit: true,
    fitRevision: contentBoundsKey(),
    fit: camera => { if (!contentBounds) return false; camera.fitToView(contentBounds, { maxScale: AUTO_FIT_MAX_SCALE }); },
    minScale: MIN_SCALE,
    maxScale: MAX_SCALE,
    fitPadding: FIT_PADDING,
    ignoreSelector: "[data-peer-card]",
    wheelBehavior: shown === "graph" ? "zoom" : "pan",
    touch: touch && shown === "graph",
  });
  const { camera } = stage;
  const { transform, handlers, fitToView, isDragging, containerRef, layerRef } = camera;
  const zoomBy = stage.zoom;
  function contentBoundsKey() { return [width, height, webHeight, ...machineHeights].join(":"); }

  const contentBounds = useMemo(() => {
    if (!layout.length) return null;
    const minX = Math.min(gateway.x - geometry.web.width / 2, ...layout.map((p) => p.x - CARD_HALF_WIDTH));
    const maxX = Math.max(gateway.x + geometry.web.width / 2, ...layout.map((p) => p.x + CARD_HALF_WIDTH));
    const minY = Math.min(gateway.y - webHeight / 2, ...layout.map((p) => p.y - p.height / 2));
    const maxY = Math.max(gateway.y + webHeight / 2, ...layout.map((p) => p.y + p.height / 2));
    const halfWidth = Math.max(gateway.x - minX, maxX - gateway.x);
    const halfHeight = Math.max(gateway.y - minY, maxY - gateway.y);
    return { x: gateway.x - halfWidth, y: gateway.y - halfHeight, width: halfWidth * 2, height: halfHeight * 2 };
  }, [layout, gateway.x, gateway.y, geometry.web.width, webHeight, webCard, geometry.web, geometry.qrSize]);

  const reset = () => {
    if (contentBounds) fitToView(contentBounds);
  };

  const tailscaleHint = (name: string, online: boolean, identity?: import("./networkTransport").NetworkIdentity) => tailscale && (differentNetwork(tailscale.identity, identity) || suggestsRemoteTailscale(online, tailscale.state))
    ? <TailscaleMachineHint name={name} tailscale={tailscale} detail={differentNetwork(tailscale.identity, identity) ? `${name} is signed in to a different Tailscale account. Use the same account on both computers.` : undefined} />
    : undefined;

  const peerFooter = (id: string) => {
    const footer = account?.canvas.peerFooters.get(id);
    if (!account || !footer) {
      const peer = peers.find((candidate) => candidate.instance_id === id);
      return peer ? <MachineFooter label={peer.pending ? "Waiting to pair" : peer.online ? "Connected" : "Unreachable"}
        secondary={peer.reachabilityHint ?? (peer.pending ? "pairs when online" : peer.online ? ACCESS_LABELS[peer.access_level] : undefined)}
        tone={peer.online ? "ok" : "warning"} version={peer.domios_version} build={peer.domios_build} /> : undefined;
    }
    const peer = peers.find((candidate) => candidate.instance_id === id);
    const name = footer.row.name;
    const seen = peer && !peer.online ? lastSeenMs(peer.last_seen) : null;
    return <PeerAccountFooterView peer={peer} footer={footer} name={name} lastSeen={seen === null ? null : formatAge(seen, account.now)}
      busy={account.busy === footer.key} actions={account.actions} hint={peer?.reachabilityHint ?? (!offersTailscaleSetup(footer.row.status) ? undefined : tailscaleHint(name, peer?.online ?? false, footer.row.transport))} />;
  };

  const machineCard = (p: PeerLayout) => {
    if (p.cloud && cloud) {
      if (p.cloud.kind === "slot") {
        return (
          <div style={{ transform: `scale(${MACHINE_CARD_SCALE})` }}>
            <CloudMachineSlot
              usage={cloud.usage}
              isFree={cloud.isFree}
              onCreateCloud={cloud.onCreateCloud}
              onPairComputer={cloud.onPairComputer}
              onCancel={cloud.onCloseSlot}
            />
          </div>
        );
      }
      if (p.cloud.kind === "pending") {
        const pending = p.cloud.pending;
        return (
          <div style={{ transform: `scale(${MACHINE_CARD_SCALE})` }}>
            <CloudMachineCard
              machine={{
                machine_id: "pending",
                name: "Cloud machine",
                size: pending.size,
                state: pending.status === "failed" ? "failed" : "creating",
              }}
              usage={cloud.usage}
              failure={pending.failure ?? null}
              onFailureAction={cloud.onFailureAction}
            />
          </div>
        );
      }
      const { machine, peer } = p.cloud;
      const row = peer ? account?.canvas.peerFooters.get(peer.instance_id)?.row : undefined;
      const onRename = account?.actions.onRename;
      return (
        <div style={{ transform: `scale(${MACHINE_CARD_SCALE})` }}>
          <CloudMachineCard
            machine={machine}
            usage={cloud.usage}
            version={peer?.domios_version}
            build={peer?.domios_build}
            reachabilityHint={peer?.reachabilityHint}
            selected={cloud.selectedMachineId === machine.machine_id}
            onClick={() => cloud.onSelectMachine(machine.machine_id)}
            onTopUp={() => cloud.onTopUpMachine(machine)}
            onRename={row?.target && onRename ? (name) => onRename(row, name) : undefined}
            renameUnavailable={!row || !row.target || !peer?.online}
          />
        </div>
      );
    }
    const row = account?.canvas.peerFooters.get(p.id)?.row;
    const onRename = account?.actions.onRename;
    return (
      p.account && account ? (
        p.account.kind === "node"
          ? <AccountMachineCard node={p.account.node} busy={account.busy === p.account.node.key} now={account.now} actions={account.actions}
            hint={offersTailscaleSetup(p.account.node.row.status) && p.account.node.row.target ? tailscaleHint(p.account.node.name, p.account.node.online === true, p.account.node.row.transport) : undefined} />
          : <AccountStateCard card={p.account.card} signingIn={account.signingIn} onSignIn={account.onSignIn} onCancel={account.cancelSignIn} onRetry={account.onRetry} />
      ) : (
        <div style={{ transform: `scale(${MACHINE_CARD_SCALE})` }}>
        <PeerTerminal
          peerId={p.isThisMachine ? undefined : p.id}
          sameNameOthers={p.isThisMachine ? account?.selfSameNameOthers : undefined}
          displayName={p.isThisMachine ? account?.selfDisplayName : row?.displayName}
          onRename={p.isThisMachine ? account?.onRenameSelf : row?.target && onRename ? (name) => onRename(row, name) : undefined}
          renameUnavailable={row && (!row.target || !row.online || !peers.find((peer) => peer.instance_id === p.id)?.online)}
          footer={p.isThisMachine ? <>
            {account?.limitMachines && <MachineLimitPaywall machines={account.limitMachines} />}
            {account?.removed && <div role="group" aria-label="Rejoin your mesh" onClick={(event) => event.stopPropagation()} className="px-3.5 py-2.5">
              <p role="status">{REMOVED_COPY}</p>
              <Button size="compact" loading={account.signingIn} disabled={!!account.busy} onClick={account.onRejoin}>{account.signingIn ? "Signing in…" : "Sign in and rejoin"}</Button>
              {account.cancelSignIn && <Button size="compact" onClick={account.cancelSignIn}>Cancel</Button>}
            </div>}
            {account?.reauthRequired && !account.removed && <MachineReauthentication signingIn={account.signingIn} onSignIn={account.onSignIn} onCancel={account.cancelSignIn} />}
            <LocalMachineFooter state={tailscale?.state} />
            <DuplicateMachineNameHint name={p.name} others={account?.selfSameNameOthers} className="px-3.5 pb-2"
              rename={account?.onRenameSelf ? { displayName: account.selfDisplayName, save: account.onRenameSelf } : undefined} />
            {tailscale && tailscale.state.kind !== "connected" && <TailscaleFooter tailscale={tailscale} />}
          </> : peerFooter(p.id)}
          inventory={peerInventoryModel(services, peers.find((peer) => peer.instance_id === p.id) ?? { instance_id: p.id, name: p.name, online: true }, !!p.isThisMachine, loading, error)}
          name={p.name}
          ip={p.ip}
          isThisMachine={p.isThisMachine}
          disabled={row?.status === "signed_out"}
          onClick={() => onPeerClick(p.isThisMachine ? LOCAL_MACHINE_ID : p.id)}
          offline={p.isThisMachine ? !meshEnabled : row?.status === "signed_out" || !peers.find((peer) => peer.instance_id === p.id)?.online}
          onGoOnline={p.isThisMachine ? onGoOnline : undefined}
        />
        </div>
      )
    );
  };

  const viewToggle = (
    <MeshViewToggle
      value={viewState.viewMode}
      onChange={viewState.selectViewMode}
      iconOnly={iconOnlyToggle}
      touch={touch}
    />
  );

  const accountActions = <>
    {account?.onRetryEnrollment && <button type="button" onMouseDown={(event) => event.stopPropagation()}
      disabled={account.busy !== null} aria-busy={account.busy === "enrollment"}
      onClick={account.onRetryEnrollment} className="ct-btn ct-btn-compact absolute bottom-3 right-3 z-10">Retry enrollment</button>}
    {account?.moreMachines && <button type="button" onMouseDown={(event) => event.stopPropagation()}
      onClick={account.onRetry} className="ct-btn ct-btn-compact absolute bottom-3 left-3 z-10">Load more machines</button>}
  </>;

  const addMachinePill = cloud && !showCloudSlot
    ? <button type="button" data-testid="mesh-add-machine" className="ct-bar-pill gap-1.5" onClick={cloud.onOpenSlot}>
      <Plus size={13} aria-hidden="true" />{iconOnlyToggle ? "Add" : "Add machine"}
    </button>
    : undefined;

  const overview = account && meshOverviewState(account.canvas, peers, tailscale?.state, account.moreMachines);
  const overviewAvailable = overview && overview.kind !== "inventory" && cloudMachines.length === 0 && !showCloudSlot;
  if (account && overview && overviewAvailable && !showInventory) {
    return <section aria-label="Mesh inventory" className="mesh-overview ct-canvas-background relative flex h-full w-full overflow-auto">
      {(overview.kind === "unreachable" || overview.kind === "tailscale_off" || cloud) &&
        <CanvasToolbar label="Mesh inventory controls" onRefresh={onRefresh}
          view={<>{addMachinePill}{(overview.kind === "unreachable" || overview.kind === "tailscale_off") && <button type="button" className="ct-bar-pill" aria-label="Show inventory" onClick={() => setShowInventory(true)}>
            <Network size={13} aria-hidden="true" />
          </button>}</>} />}
      <MeshOverview state={overview} account={account} name={account.selfName ?? thisMachine.name} tailscale={tailscale} />
      <AccountNotes notes={account.canvas.notes} />
      {accountActions}
    </section>;
  }
  const inventoryToggle = (
    <>
      {addMachinePill}
      {viewToggle}
      {overviewAvailable && <button type="button" className="ct-bar-pill" onClick={() => setShowInventory(false)}>Overview</button>}
    </>
  );

  if (shown === "list") {
    const selectedId = cloud?.selectedMachineId ? `cloud:${cloud.selectedMachineId}` : selectedPeerId;
    return (
      <MeshMachineList
        toolbarView={inventoryToggle}
        webCard={webCard}
        webSelected={webSelected}
        webStatus={webStatus}
        onWebClick={onWebClick}
        thisMachine={thisMachine}
        peers={peers}
        account={account}
        cloud={cloud}
        tailscale={tailscale}
        meshEnabled={meshEnabled}
        width={width}
        height={height}
        touch={touch}
        selectedId={selectedId}
        collapsedOverrides={viewState.collapsedOverrides}
        onToggleGroup={viewState.toggleGroup}
        onPeerClick={onPeerClick}
        onRefresh={onRefresh}
        onGoOnline={onGoOnline}
        onOpenPeerWebUi={onOpenPeerWebUi}
        webUiBaseUrl={webUiBaseUrl}
        webUiToken={webUiToken}
      />
    );
  }

  return (
    <div
      ref={containerRef}
      {...stage.interactions}
      tabIndex={0}
      className="ct-canvas-background relative w-full h-full overflow-hidden"
      onMouseDown={handlers.onMouseDown}
      style={{
        minHeight: height,
        cursor: isDragging ? "grabbing" : "grab",
        userSelect: "none",
        touchAction: "none",
      }}
    >
      {account && <AccountNotes notes={account.canvas.notes} />}
      {accountActions}
      <CanvasToolbar label="Mesh graph controls" view={inventoryToggle} onRefresh={onRefresh} onFit={() => { stage.takeControl(); reset(); }} onZoomOut={() => zoomBy(-zoomStep)} onZoomIn={() => zoomBy(zoomStep)} />
      <div
        ref={(element) => { layerRef.current = element; machineLayerRef.current = element; }}
        style={{
          transform: formatLayerTransform(transform),
          transformOrigin: "0 0",
          width: width,
          height: height,
          position: "relative",
        }}
      >
        <svg
          className="absolute inset-0 pointer-events-none"
          width={width}
          height={height}
          style={{ overflow: "visible" }}
        >
          {gatewayGraph.edges.map((edge) => {
            const machine = layout.find((item) => item.id === edge.machineId);
            if (!machine) return null;
            const signedOut = account?.canvas.peerFooters.get(edge.machineId)?.row.status === "signed_out";
            const paint = meshEdgeModel(geometry.web, { ...machine, width: MACHINE_CARD_WIDTH }, edge.online && !signedOut);
            const label = `${machine.name}: ${signedOut ? "Signed out" : edge.online ? "online" : "offline"} ${edge.label}`;
            const launch = () => {
              if (machine.isThisMachine) onWebClick?.();
              else if (machine.webUrl) onOpenPeerWebUi?.(machine.webUrl);
            };
            return (
              <g key={edge.machineId} role="button" tabIndex={0} aria-label={label} aria-disabled={!machine.isThisMachine && !machine.webUrl}
                style={{ pointerEvents: "auto", cursor: "pointer" }}
                onClick={launch}
                onKeyDown={(event) => {
                  if (event.code === "Enter" || event.code === "Space") { event.preventDefault(); launch(); }
                }}>
                <title>{label}</title>
                <path d={paint.path} fill="none" stroke="var(--ct-muted-fg)" strokeWidth={1.2} strokeLinecap="round" opacity={paint.opacity} strokeDasharray={paint.dash} />
              </g>
            );
          })}
          {layout.filter((item) => item.cloud).map((item) => {
            const entry = item.cloud!;
            const edgeKind = entry.kind === "slot"
              ? "dashed"
              : entry.kind === "pending"
                ? entry.pending.status === "failed" ? "dashed" : "preparing"
                : cloudMachineCardView(entry.machine, cloud?.usage).edgeKind;
            const paint = meshEdgeModel(geometry.web, { ...item, width: MACHINE_CARD_WIDTH }, edgeKind === "solid");
            return (
              <path
                key={item.id}
                d={paint.path}
                fill="none"
                stroke={edgeKind === "preparing" ? "var(--ct-data-flow)" : "var(--ct-muted-fg)"}
                strokeWidth={edgeKind === "preparing" ? 1.5 : 1.2}
                strokeLinecap="round"
                opacity={edgeKind === "preparing" ? 0.9 : paint.opacity}
                strokeDasharray={edgeKind === "preparing" ? "4 6" : paint.dash}
                className={edgeKind === "preparing" ? "ct-cloud-edge--preparing" : undefined}
              />
            );
          })}

        </svg>
        <div ref={webCardRef} data-peer-card className="absolute" style={{ left: gateway.x, top: gateway.y, width: geometry.web.width, transform: "translate(-50%,-50%)" }}>
          <div style={{ zoom: WEB_CARD_SCALE, width: geometry.web.width / WEB_CARD_SCALE }}>
            {webCard ? webCard(geometry.qrSize / WEB_CARD_SCALE) : <button type="button" aria-label="Web" aria-pressed={webSelected} onClick={onWebClick}
              className="w-full rounded-xl border border-[var(--ct-accent)] bg-[var(--ct-bg)] p-4">
              <Globe size={24} aria-hidden="true" /><strong>Web</strong><span>{webStatus}</span>
            </button>}
          </div>
        </div>
        {layout.map((p, i) => (
          <div
            key={`${p.id}-${i}`}
            data-peer-card
            data-machine-index={i}
            style={{
              position: "absolute",
              left: p.x,
              top: p.y,
              transform: "translate(-50%,-50%)",
            }}
          >
            {machineCard(p)}
          </div>
        ))}
      </div>
      {viewState.suggestionVisible && machineCount !== null && (
        <MeshListSuggestionBanner
          count={machineCount}
          onSwitchToList={viewState.acceptSuggestion}
          onKeepCanvas={viewState.keepCanvas}
        />
      )}
    </div>
  );
});
