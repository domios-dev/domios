import { REMOVED_COPY } from "./accountMachines";
import { peerInventoryModel } from "./peerInventoryModel";
import type { Service } from "./types";
import { memo, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { Globe, List, Network } from "lucide-react";
import { PeerTerminal } from "./PeerTerminal";
import { lastSeenMs, type AccountCanvas, type AccountCanvasCard, type AccountMachineNode } from "./accountCanvas";
import { AccountMachineCard, AccountNotes, AccountStateCard, MachineReauthentication, PeerAccountFooterView, type MachineNodeActions } from "./AccountCanvasNodes";
import { formatAge } from "../../lib/timeLabels";
import { meshEdgeModel } from "./meshEdgeModel";
import type { PeerStatus } from "./useMeshData";
import { CanvasToolbar } from "@codeterm/chat-ui/CanvasToolbar";
import { useCanvasViewport, formatLayerTransform, AUTO_FIT_MAX_SCALE } from "@codeterm/canvas-camera";
import { webLinkWithTokenAndRelay } from "../../lib/webAccess";
import { RELAY_PROTOCOL_VERSION } from "../../lib/types";
import { webGraphLayout, webGraphInitialView, WEB_CARD_SCALE, MACHINE_CARD_SCALE, MACHINE_CARD_WIDTH } from "./webGraphLayout";
import { webGatewayGraph } from "./meshProjection";
import { TailscaleFooter, TailscaleMachineHint, type CanvasTailscale } from "./TailscaleNodes";
import { differentNetwork } from "./networkTransport";
import { offersTailscaleSetup, suggestsRemoteTailscale } from "./tailscaleSetup";
import { MachineLimitPaywall } from "./MachineLimitPaywall";
import type { MyMachines } from "./useMyMachines";
import { inventoryView, listCardScale, meshZoomStep, offersInventoryList, type InventoryView } from "./meshViewportModel";

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
  webUrl?: string | null;
  webUnavailableReason?: string;
}

/** My machines on the canvas: account nodes, paired-card footers and the actions behind them. */
export interface CanvasAccount {
  selfName?: string;
  onRenameSelf?: (name: string) => Promise<boolean>;
  canvas: AccountCanvas;
  busy: string | null;
  signingIn: boolean;
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
  onPeerClick: (peerName: string) => void;
  width: number;
  height: number;
  account?: CanvasAccount;
  tailscale?: CanvasTailscale;
  meshEnabled?: boolean;
  onGoOnline?: () => void;
  onOpenPeerWebUi?: (url: string) => void;
  webUiBaseUrl?: string | null;
  webUiToken?: string | null;
  /** A touch screen: pinch and pan the graph, bigger +/- steps, and on a phone the stacked list first. */
  touch?: boolean;
}

const MIN_SCALE = 0.001;
const MAX_SCALE = 2.5;
/** The QR in the list's Mesh UI card, at the size the graph card shows it unscaled. */
const LIST_QR_SIZE = 160;
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
  tailscale,
  meshEnabled = true,
  onGoOnline,
  onOpenPeerWebUi,
  webUiBaseUrl = null,
  webUiToken = null,
  touch = false,
}: Props) {
  const [chosenView, setChosenView] = useState<InventoryView | null>(null);
  const listOffered = offersInventoryList(width, touch);
  const shown = inventoryView(width, touch, chosenView);
  const zoomStep = meshZoomStep(touch);
  const webCardRef = useRef<HTMLDivElement>(null);
  const [webHeight, setWebHeight] = useState(800);
  const [machineHeights, setMachineHeights] = useState<number[]>([]);
  const machineLayerRef = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const element = webCardRef.current;
    if (!element) return;
    if (element.offsetHeight > 0) setWebHeight(element.offsetHeight);
    const observer = new ResizeObserver(() => { if (element.offsetHeight > 0) setWebHeight(element.offsetHeight); });
    observer.observe(element);
    return () => observer.disconnect();
  }, [shown]);
  const accountNodes = account?.canvas.nodes;
  const accountCard = account?.canvas.card ?? null;
  const extraCount = (accountNodes?.length ?? 0) + (accountCard ? 1 : 0);
  const geometry = webGraphLayout(width, height, peers.length + 1 + extraCount, webHeight, machineHeights);
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
    peers.forEach((p, i) => {
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
    for (const node of accountNodes ?? []) {
      items.push({ id: `account:${node.key}`, name: node.name, ip: "", ...geometry.machines[items.length], account: { kind: "node", node } });
    }
    if (accountCard) {
      items.push({ id: `account-card:${accountCard.kind}`, name: "My machines", ip: "", ...geometry.machines[items.length], account: { kind: "card", card: accountCard } });
    }
    return items;
  }, [thisMachine, peers, account, accountNodes, accountCard, webUiBaseUrl, webUiToken, geometry.machines]);

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
  const gatewayGraph = webGatewayGraph(peers, meshEnabled);
  const { transform, handlers, fitToView, zoomBy, isDragging, containerRef, layerRef } = useCanvasViewport({
    width,
    height,
    minScale: MIN_SCALE,
    maxScale: MAX_SCALE,
    fitPadding: FIT_PADDING,
    ignoreSelector: "[data-peer-card]",
    wheelBehavior: "zoom",
    touch: touch && shown === "graph",
  });
  const userAdjustedRef = useRef(false);
  const fittedBoundsRef = useRef("");

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

  useLayoutEffect(() => {
    if (!contentBounds) return;
    const key = [width, height, contentBounds.x, contentBounds.y, contentBounds.width, contentBounds.height].join(":");
    const initial = webGraphInitialView(width, height, contentBounds, userAdjustedRef.current);
    if (!initial || fittedBoundsRef.current === key) return;
    fittedBoundsRef.current = key;
    fitToView(initial.bounds, { maxScale: AUTO_FIT_MAX_SCALE });
  }, [fitToView, contentBounds, width, height]);

  const reset = () => {
    if (contentBounds) fitToView(contentBounds);
  };

  const tailscaleHint = (name: string, online: boolean, identity?: import("./networkTransport").NetworkIdentity) => tailscale && (differentNetwork(tailscale.identity, identity) || suggestsRemoteTailscale(online, tailscale.state))
    ? <TailscaleMachineHint name={name} tailscale={tailscale} detail={differentNetwork(tailscale.identity, identity) ? `${name} is signed in to a different Tailscale account. Use the same account on both computers.` : undefined} />
    : undefined;

  const peerFooter = (id: string) => {
    const footer = account?.canvas.peerFooters.get(id);
    if (!account || !footer) return undefined;
    const peer = peers.find((candidate) => candidate.instance_id === id);
    const name = footer.row.name;
    const seen = peer && !peer.online ? lastSeenMs(peer.last_seen) : null;
    return <PeerAccountFooterView footer={footer} name={name} lastSeen={seen === null ? null : formatAge(seen, account.now)}
      busy={account.busy === footer.key} actions={account.actions} hint={!offersTailscaleSetup(footer.row.status) ? undefined : tailscaleHint(name, peer?.online ?? false, footer.row.transport)} />;
  };

  const machineCard = (p: PeerLayout) => {
    const row = account?.canvas.peerFooters.get(p.id)?.row;
    const onRename = account?.actions.onRename;
    return (
      p.account && account ? (
        p.account.kind === "node"
          ? <AccountMachineCard node={p.account.node} busy={account.busy === p.account.node.key} now={account.now} actions={account.actions}
            hint={offersTailscaleSetup(p.account.node.row.status) && p.account.node.row.target ? tailscaleHint(p.account.node.name, p.account.node.online === true, p.account.node.row.transport) : undefined} />
          : <AccountStateCard card={p.account.card} signingIn={account.signingIn} onSignIn={account.onSignIn} onRetry={account.onRetry} />
      ) : (
        <div style={{ transform: `scale(${MACHINE_CARD_SCALE})` }}>
        <PeerTerminal
          onRename={p.isThisMachine ? account?.onRenameSelf : row?.target && onRename ? (name) => onRename(row, name) : undefined}
          renameDisabledReason={row && (row.status === "signed_out" || !peers.find((peer) => peer.instance_id === p.id)?.online)
            ? "Connect to rename it, or rename it on that machine" : undefined}
          footer={p.isThisMachine ? <>
            {account?.limitMachines && <MachineLimitPaywall machines={account.limitMachines} />}
            {account?.removed && <div role="group" aria-label="Rejoin your mesh" onClick={(event) => event.stopPropagation()} className="px-3.5 py-2.5">
              <p role="status">{REMOVED_COPY}</p>
              <button type="button" disabled={!!account.busy || account.signingIn} onClick={account.onRejoin}>Sign in and rejoin</button>
            </div>}
            {account?.reauthRequired && !account.removed && <MachineReauthentication signingIn={account.signingIn} onSignIn={account.onSignIn} />}
            {tailscale && <TailscaleFooter tailscale={tailscale} />}
          </> : peerFooter(p.id)}
          inventory={peerInventoryModel(services, peers.find((peer) => peer.instance_id === p.id) ?? { instance_id: p.id, name: p.name, online: true }, !!p.isThisMachine, loading, error)}
          name={p.name}
          ip={p.ip}
          isThisMachine={p.isThisMachine}
          isPaired={p.isPaired}
          pending={p.pending}
          accessLevel={p.accessLevel as "full" | "read_only" | "agent_isolated" | undefined}
          disabled={row?.status === "signed_out"}
          onClick={() => onPeerClick(p.isThisMachine ? thisMachine.name : peers.find((peer) => peer.instance_id === p.id)?.name ?? p.name)}
          offline={p.isThisMachine ? !meshEnabled : row?.status === "signed_out" || !peers.find((peer) => peer.instance_id === p.id)?.online}
          onGoOnline={p.isThisMachine ? onGoOnline : undefined}
        />
        </div>
      )
    );
  };

  const viewToggle = listOffered
    ? <button type="button" data-testid="mesh-inventory-view-toggle" className="ct-bar-pill gap-1.5"
      aria-label={shown === "list" ? "Show as graph" : "Show as list"}
      onClick={() => setChosenView(shown === "list" ? "graph" : "list")}>
      {shown === "list" ? <><Network size={13} aria-hidden="true" />Graph</> : <><List size={13} aria-hidden="true" />List</>}
    </button>
    : undefined;

  const accountActions = <>
    {account?.onRetryEnrollment && <button type="button" onMouseDown={(event) => event.stopPropagation()}
      disabled={account.busy !== null} aria-busy={account.busy === "enrollment"}
      onClick={account.onRetryEnrollment} className="ct-btn ct-btn-compact absolute bottom-3 right-3 z-10">Retry enrollment</button>}
    {account?.moreMachines && <button type="button" onMouseDown={(event) => event.stopPropagation()}
      onClick={account.onRetry} className="ct-btn ct-btn-compact absolute bottom-3 left-3 z-10">Load more machines</button>}
  </>;

  if (shown === "list") {
    const scale = listCardScale(width);
    return (
      <div className="relative flex h-full w-full flex-col overflow-hidden" style={{ minHeight: height, background: "var(--ct-bg)" }}>
        <div className="shrink-0 px-3 pb-2 pt-3">
          <CanvasToolbar floating={false} label="Mesh inventory controls" view={viewToggle} onRefresh={onRefresh} />
        </div>
        <div role="list" aria-label="Machines" data-testid="mesh-inventory-list"
          className="min-h-0 flex-1 overflow-y-auto overscroll-contain px-3 pb-16" style={{ touchAction: "pan-y" }}>
          <div className="mx-auto flex flex-col items-center gap-3">
            {layout.map((p, i) => (
              <div key={`${p.id}-${i}`} role="listitem" data-peer-card style={{ zoom: scale }}>{machineCard(p)}</div>
            ))}
            {/* On a phone the user is usually already in the web UI, so its QR card follows the machines. */}
            <div role="listitem" ref={webCardRef} data-peer-card style={{ zoom: scale, width: MACHINE_CARD_WIDTH }}>
              {webCard ? webCard(LIST_QR_SIZE) : <button type="button" aria-label="Web" aria-pressed={webSelected} onClick={onWebClick}
                className="w-full rounded-xl border border-[var(--ct-accent)] bg-[var(--ct-bg)] p-4">
                <Globe size={24} aria-hidden="true" /><strong>Web</strong><span>{webStatus}</span>
              </button>}
            </div>
          </div>
        </div>
        {account && <AccountNotes notes={account.canvas.notes} />}
        {accountActions}
      </div>
    );
  }

  return (
    <div
      ref={containerRef}
      className="relative w-full h-full overflow-hidden"
      onMouseDown={(event) => { userAdjustedRef.current = true; handlers.onMouseDown(event); }}
      onWheelCapture={() => { userAdjustedRef.current = true; }}
      onTouchStartCapture={() => { userAdjustedRef.current = true; }}
      style={{
        minHeight: height,
        background: "var(--ct-bg)",
        cursor: isDragging ? "grabbing" : "grab",
        userSelect: "none",
        touchAction: "none",
      }}
    >
      {account && <AccountNotes notes={account.canvas.notes} />}
      {accountActions}
      <CanvasToolbar label="Mesh graph controls" view={viewToggle} onRefresh={onRefresh} onFit={() => { userAdjustedRef.current = true; reset(); }} onZoomOut={() => { userAdjustedRef.current = true; zoomBy(-zoomStep); }} onZoomIn={() => { userAdjustedRef.current = true; zoomBy(zoomStep); }} />
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
    </div>
  );
});
