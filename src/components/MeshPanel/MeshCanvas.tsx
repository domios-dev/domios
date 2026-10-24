import { peerInventoryModel } from "./peerInventoryModel";
import type { Service } from "./types";
import { memo, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { Globe } from "lucide-react";
import { PeerTerminal } from "./PeerTerminal";
import { lastSeenMs, type AccountCanvas, type AccountCanvasCard, type AccountMachineNode } from "./accountCanvas";
import { AccountMachineCard, AccountNotes, AccountStateCard, MachineReauthentication, PeerAccountFooterView, type MachineNodeActions } from "./AccountCanvasNodes";
import { formatAge } from "../../lib/timeLabels";
import { meshEdgeModel } from "./meshEdgeModel";
import type { PeerStatus } from "./useMeshData";
import { CanvasToolbar } from "@codeterm/chat-ui";
import { useCanvasViewport, formatLayerTransform } from "@codeterm/canvas-camera";
import { webLinkWithTokenAndRelay } from "../../lib/webAccess";
import { RELAY_PROTOCOL_VERSION } from "../../lib/types";
import { webGraphLayout, webGraphInitialView, WEB_CARD_SCALE, MACHINE_CARD_SCALE, MACHINE_CARD_WIDTH } from "./webGraphLayout";
import { webGatewayGraph } from "./meshProjection";
import { TailscaleFooter, TailscaleMachineHint, type CanvasTailscale } from "./TailscaleNodes";
import { suggestsRemoteTailscale } from "./tailscaleSetup";

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
  moreMachines?: boolean;
  /** Epoch milliseconds. */
  now: number;
  actions: MachineNodeActions;
  onSignIn: () => void;
  onRetry: () => void;
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
}

const MIN_SCALE = 0.001;
const MAX_SCALE = 2.5;
const ZOOM_STEP = 0.06;
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
}: Props) {
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
  }, []);
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
  }, [layout.length]);

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
    fitToView(initial.bounds);
  }, [fitToView, contentBounds, width, height]);

  const reset = () => {
    if (contentBounds) fitToView(contentBounds);
  };

  const tailscaleHint = (name: string, online: boolean) => tailscale && suggestsRemoteTailscale(online, tailscale.state)
    ? <TailscaleMachineHint name={name} tailscale={tailscale} />
    : undefined;

  const peerFooter = (id: string) => {
    const footer = account?.canvas.peerFooters.get(id);
    if (!account || !footer) return undefined;
    const peer = peers.find((candidate) => candidate.instance_id === id);
    const name = footer.row.name;
    const seen = peer && !peer.online ? lastSeenMs(peer.last_seen) : null;
    return <PeerAccountFooterView footer={footer} name={name} lastSeen={seen === null ? null : formatAge(seen, account.now)}
      busy={account.busy === footer.key} actions={account.actions} hint={footer.row.status === "signed_out" ? undefined : tailscaleHint(name, peer?.online ?? false)} />;
  };

  return (
    <div
      ref={containerRef}
      className="relative w-full h-full overflow-hidden"
      onMouseDown={(event) => { userAdjustedRef.current = true; handlers.onMouseDown(event); }}
      onWheelCapture={() => { userAdjustedRef.current = true; }}
      onTouchStartCapture={() => { userAdjustedRef.current = true; }}
      style={{
        minHeight: height,
        background:
          "radial-gradient(ellipse at center, var(--ct-titlebar-bg) 0%, var(--ct-bg) 100%)",
        cursor: isDragging ? "grabbing" : "grab",
        userSelect: "none",
        touchAction: "none",
      }}
    >
      {account && <AccountNotes notes={account.canvas.notes} />}
      {account?.moreMachines && <button type="button" onMouseDown={(event) => event.stopPropagation()}
        onClick={account.onRetry} className="ct-btn ct-btn-compact absolute bottom-3 left-3 z-10">Load more machines</button>}
      <CanvasToolbar label="Mesh graph controls" onRefresh={onRefresh} onFit={() => { userAdjustedRef.current = true; reset(); }} onZoomOut={() => { userAdjustedRef.current = true; zoomBy(-ZOOM_STEP); }} onZoomIn={() => { userAdjustedRef.current = true; zoomBy(ZOOM_STEP); }} />
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
        {layout.map((p, i) => {
          const row = account?.canvas.peerFooters.get(p.id)?.row;
          const onRename = account?.actions.onRename;
          return (
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
            {p.account && account ? (
              p.account.kind === "node"
                ? <AccountMachineCard node={p.account.node} busy={account.busy === p.account.node.key} now={account.now} actions={account.actions}
                  hint={p.account.node.row.status !== "signed_out" && p.account.node.row.target ? tailscaleHint(p.account.node.name, p.account.node.online !== false) : undefined} />
                : <AccountStateCard card={p.account.card} signingIn={account.signingIn} onSignIn={account.onSignIn} onRetry={account.onRetry} />
            ) : (
              <div style={{ transform: `scale(${MACHINE_CARD_SCALE})` }}>
              <PeerTerminal
                onRename={p.isThisMachine ? account?.onRenameSelf : row?.target && onRename ? (name) => onRename(row, name) : undefined}
                renameDisabledReason={row && (row.status === "signed_out" || !peers.find((peer) => peer.instance_id === p.id)?.online)
                  ? "Connect to rename it, or rename it on that machine" : undefined}
                footer={p.isThisMachine ? <>
                  {account?.reauthRequired && <MachineReauthentication signingIn={account.signingIn} onSignIn={account.onSignIn} />}
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
            )}
          </div>
          );
        })}
      </div>
    </div>
  );
});
