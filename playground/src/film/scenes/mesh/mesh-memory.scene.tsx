import { Search } from "lucide-react";
import { ConnectionLine } from "../../../../../src/components/MeshPanel/ConnectionLine";
import { MemoryListTab } from "../../../../../src/components/MemoryPanel/MemoryListTab";
import type { MemKind, MemNode } from "../../../../../src/components/MemoryPanel/types";
import { API } from "../../../gallery/fixtures";
import { Camera, easeOut, range, useStage, type Key, type Scene } from "../../engine";
import { Appear, At, H, MachineCard, meshBackground, PH, Pulse, PW, Ripple, Typed, W, type PeerName } from "./meshFixtures";

const ORDER: PeerName[] = ["mac-mini", "homelab", "gpu-rig"];

type Point = [number, number];
type Panel = { left: number; top: number; width: number; height: number; scale: number; listTop: number; headerFont: number; searchFont: number; askTop: number; askFont: number; compact?: boolean };
type Layout = {
  width: number;
  height: number;
  camera: Key[];
  peers: Array<{ peer: PeerName; x: number; y: number; edge: Point }>;
  card: number;
  nameSize: number;
  panel: Panel;
  inlet: Point;
  pulse: number;
};

const L_PEER_X = 360;
const L_PANEL: Panel = { left: 860, top: 120, width: 920, height: 840, scale: 1.6, listTop: 150, headerFont: 26, searchFont: 28, askTop: 380, askFont: 26 };

const LANDSCAPE: Layout = {
  width: W,
  height: H,
  camera: [{ t: 0, x: 945, y: 540, scale: 1.07 }, { t: 6, x: 960, y: 540, scale: 1.1 }],
  peers: ORDER.map((peer, i) => ({ peer, x: L_PEER_X, y: 230 + i * 310, edge: [L_PEER_X + 234, 230 + i * 310] })),
  card: 1.3,
  nameSize: 17,
  panel: L_PANEL,
  inlet: [L_PANEL.left, H / 2],
  pulse: 9,
};

const P_CARD_Y = 220;
const P_PANEL: Panel = { left: 84, top: 480, width: 912, height: 950, scale: 2.2, listTop: 168, headerFont: 30, searchFont: 30, askTop: 440, askFont: 28, compact: true };

const PORTRAIT: Layout = {
  width: PW,
  height: PH,
  camera: [{ t: 0, x: PW / 2, y: PH / 2, scale: 1.0 }, { t: 6, x: PW / 2, y: PH / 2, scale: 1.02 }],
  peers: ORDER.map((peer, i) => ({ peer, x: 240 + i * 300, y: P_CARD_Y, edge: [240 + i * 300, P_CARD_Y + 72] })),
  card: 0.78,
  nameSize: 34,
  panel: P_PANEL,
  inlet: [P_PANEL.left + P_PANEL.width / 2, P_PANEL.top],
  pulse: 9,
};

const QUERY = "tailscale https cert renewal";
const QUERY_AT = -0.4;
const ASK_AT = 0.75;
const ASK_LAND = 1.4;
const TRAVEL = 0.75;
const ANSWERED_AT = 3.9;

type Hit = { peer: PeerName; kind: MemKind; title: string; area: string; ageHours: number; arrive: number };

const HITS: Hit[] = [
  { peer: "mac-mini", kind: "fact", title: "tailscale serve renews the gateway HTTPS cert on its own", area: "infra", ageHours: 30, arrive: 1.55 },
  { peer: "gpu-rig", kind: "skill", title: "Renew a tailnet cert on Windows from an elevated shell", area: "windows", ageHours: 120, arrive: 1.85 },
  { peer: "homelab", kind: "learning", title: "Caddy needs the tailnet cert path, not the ACME one", area: "infra", ageHours: 72, arrive: 2.15 },
  { peer: "mac-mini", kind: "rule", title: "Keep cert files out of memory; store paths only", area: "security", ageHours: 400, arrive: 2.45 },
  { peer: "gpu-rig", kind: "daily", title: "Jellyfin picked up the renewed cert after a restart", area: "media", ageHours: 9, arrive: 2.75 },
  { peer: "homelab", kind: "note", title: "Grafana alert fires 14 days before the cert expires", area: "monitoring", ageHours: 200, arrive: 3.05 },
];

const NOW = Date.now();
const iso = (hoursAgo: number) => new Date(NOW - hoursAgo * 3_600_000).toISOString();

const NODE_MAPS = HITS.map((_, count) =>
  new Map<string, MemNode>(
    HITS.slice(0, count + 1).map((hit, i): [string, MemNode] => [
      `hit-${i}`,
      {
        id: `hit-${i}`,
        kind: hit.kind,
        title: hit.title,
        summary: null,
        area: hit.area,
        importance: 6,
        tags: [hit.peer],
        has_vector: true,
        has_body: true,
        created_at: iso(hit.ageHours),
        updated_at: iso(i / 60),
        access_count: 3 + ((i * 7) % 9),
      },
    ]),
  ),
);
const EMPTY = new Map<string, MemNode>();

function nodesAt(t: number) {
  const count = HITS.filter((hit) => t >= hit.arrive + TRAVEL).length;
  return count ? NODE_MAPS[count - 1] : EMPTY;
}

const answering = (peer: PeerName, t: number) => HITS.some((hit) => hit.peer === peer && t >= hit.arrive && t < hit.arrive + TRAVEL) || (t >= ASK_AT && t < ASK_LAND);

/** Accent glow on a machine while it is asked or sending a result, easing out after its last send. */
function glowOf(peer: PeerName, t: number) {
  const last = Math.max(...HITS.filter((hit) => hit.peer === peer).map((hit) => hit.arrive + TRAVEL));
  return range(t, ASK_AT + 0.3, ASK_LAND, easeOut) * (1 - range(t, last, last + 0.8));
}

const noop = () => undefined;
const PANEL_CSS = `.film-mem div:has(> div > [data-testid="memory-list-search"]), .film-mem [data-testid="memory-list-empty"] { display: none; }
.film-mem-compact [data-testid="memory-kind-filter"], .film-mem-compact [data-testid="memory-area-filter"] { display: none; }`;

function MemoryPanelFrame({ t, panel }: { t: number; panel: Panel }) {
  const nodes = nodesAt(t);
  const peers = new Set([...nodes.values()].map((n) => n.tags?.[0])).size;
  const answered = range(t, ANSWERED_AT, ANSWERED_AT + 0.4, easeOut);
  const searching = range(t, ASK_AT, ASK_AT + 0.3, easeOut) * (1 - range(t, HITS[0].arrive + TRAVEL - 0.2, HITS[0].arrive + TRAVEL));
  return (
    <div style={{ position: "absolute", left: panel.left, top: panel.top, width: panel.width, height: panel.height, borderRadius: 14, border: "1px solid var(--ct-border)", background: "var(--ct-bg)", boxShadow: "var(--ct-shadow-lg)", overflow: "hidden", zIndex: 2 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 14, padding: "22px 26px 16px", fontSize: panel.headerFont, color: "var(--ct-muted-fg)", whiteSpace: "nowrap" }}>
        <span style={{ color: "var(--ct-fg)", fontWeight: 600 }}>Memory</span>
        <span>laptop</span>
        <span style={{ marginLeft: "auto", opacity: answered, color: "var(--ct-accent)" }}>{peers} machines answered</span>
      </div>
      <div style={{ margin: "0 26px", display: "flex", alignItems: "center", gap: 14, padding: "14px 18px", borderRadius: 10, border: "1.5px solid var(--ct-accent)", fontSize: panel.searchFont, color: "var(--ct-fg)", whiteSpace: "nowrap", overflow: "hidden" }}>
        <Search size={panel.searchFont * 0.9} color="var(--ct-muted-fg)" />
        <Typed t={t} at={QUERY_AT} cps={30} text={QUERY} />
      </div>
      <style>{PANEL_CSS}</style>
      <div className={panel.compact ? "film-mem film-mem-compact" : "film-mem"} style={{ position: "absolute", left: 8, top: panel.listTop, width: (panel.width - 16) / panel.scale, height: (panel.height - panel.listTop - 8) / panel.scale, transform: `scale(${panel.scale})`, transformOrigin: "0 0" }}>
        <MemoryListTab api={API} nodes={nodes} selectedId={null} onSelectItem={noop} onViewInGraph={noop} updateNode={noop} removeNode={noop} />
      </div>
      <div style={{ position: "absolute", left: 0, right: 0, top: panel.askTop, textAlign: "center", fontSize: panel.askFont, color: "var(--ct-muted-fg)", opacity: searching }}>
        Asking mac-mini, homelab and gpu-rig…
      </div>
    </div>
  );
}

function MeshMemory({ t }: { t: number }) {
  const layout = useStage().portrait ? PORTRAIT : LANDSCAPE;
  const { peers, inlet } = layout;
  return (
    <div style={{ position: "absolute", inset: 0, background: meshBackground }}>
      <Camera t={t} width={layout.width} height={layout.height} keys={layout.camera}>
        <svg width={layout.width} height={layout.height} style={{ position: "absolute", inset: 0, overflow: "visible", zIndex: 1 }}>
          {peers.map(({ peer, edge }) => (
            <ConnectionLine key={peer} fromX={edge[0]} fromY={edge[1]} toX={inlet[0]} toY={inlet[1]} pushCount={answering(peer, t) ? 8 : 4} pulsing={false} />
          ))}
          {peers.map(({ peer, edge }) => (
            <Pulse key={`ask-${peer}`} t={t} from={inlet} to={edge} start={ASK_AT} end={ASK_LAND} r={layout.pulse * 0.65} />
          ))}
          {HITS.map((hit, i) => (
            <Pulse key={`hit-${i}`} t={t} from={peers.find((p) => p.peer === hit.peer)!.edge} to={inlet} start={hit.arrive} end={hit.arrive + TRAVEL} r={layout.pulse} />
          ))}
        </svg>
        {peers.map(({ peer, x, y }, i) => (
          <At key={peer} x={x} y={y} z={2}>
            <Appear t={t} at={-0.5 + i * 0.12}>
              <MachineCard peer={peer} scale={layout.card} nameSize={layout.nameSize} glow={glowOf(peer, t)} />
            </Appear>
          </At>
        ))}
        <MemoryPanelFrame t={t} panel={layout.panel} />
        <svg width={layout.width} height={layout.height} style={{ position: "absolute", inset: 0, overflow: "visible", zIndex: 3, pointerEvents: "none" }}>
          {peers.map(({ peer, edge }) => <Ripple key={peer} t={t} x={edge[0]} y={edge[1]} at={ASK_LAND} size={36} />)}
          {HITS.map((hit, i) => <Ripple key={i} t={t} x={inlet[0]} y={inlet[1]} at={hit.arrive + TRAVEL} size={44} />)}
        </svg>
      </Camera>
    </div>
  );
}

const meshMemory: Scene = {
  id: "mesh-memory",
  duration: 6,
  render: (t) => <MeshMemory t={t} />,
};

export default meshMemory;
