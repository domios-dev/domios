import { Canvas } from "@react-three/fiber";
import { OrbitControls } from "@react-three/drei";
import { useMemo } from "react";
import { clusterKeyFromPath, colorForCluster } from "./styles";
import type { ProjectMapNode } from "./types";

interface Props {
  nodes: ProjectMapNode[];
  focusId: number | null;
  onFocus: (id: number | null) => void;
}

interface Building {
  id: number;
  name: string;
  cluster: string;
  importance: number;
  defs: number;
}

const BLOCK_W = 2;
const PADDING = 0.6;

export function CityCanvas({ nodes, focusId, onFocus }: Props) {
  const buildings = useMemo(() => buildBuildings(nodes), [nodes]);
  const groups = useMemo(() => groupByCluster(buildings), [buildings]);

  return (
    <Canvas camera={{ position: [40, 40, 40], fov: 45 }} className="absolute inset-0">
      <ambientLight intensity={0.6} />
      <directionalLight position={[40, 60, 30]} intensity={0.8} />
      <gridHelper args={[200, 40, "#1f2933", "#11161c"]} />
      <OrbitControls makeDefault enableDamping dampingFactor={0.08} />
      {groups.map((g, i) => (
        <District
          key={g.cluster}
          cluster={g.cluster}
          buildings={g.buildings}
          origin={districtOrigin(i, groups.length)}
          focusId={focusId}
          onFocus={onFocus}
        />
      ))}
    </Canvas>
  );
}

interface DistrictProps {
  cluster: string;
  buildings: Building[];
  origin: [number, number];
  focusId: number | null;
  onFocus: (id: number | null) => void;
}

function District({ cluster, buildings, origin, focusId, onFocus }: DistrictProps) {
  const [ox, oz] = origin;
  const cols = Math.max(1, Math.ceil(Math.sqrt(buildings.length)));
  const color = colorForCluster(cluster);
  return (
    <group position={[ox, 0, oz]}>
      <mesh position={[((cols - 1) * (BLOCK_W + PADDING)) / 2, -0.05, ((cols - 1) * (BLOCK_W + PADDING)) / 2]}>
        <boxGeometry args={[cols * (BLOCK_W + PADDING) + 1, 0.05, cols * (BLOCK_W + PADDING) + 1]} />
        <meshStandardMaterial color={color} opacity={0.18} transparent />
      </mesh>
      {buildings.map((b, idx) => {
        const x = (idx % cols) * (BLOCK_W + PADDING);
        const z = Math.floor(idx / cols) * (BLOCK_W + PADDING);
        const height = Math.max(0.6, Math.min(12, 0.5 + b.importance * 800 + b.defs * 0.15));
        const isFocus = b.id === focusId;
        return (
          <mesh
            key={b.id}
            position={[x, height / 2, z]}
            onClick={(e) => {
              e.stopPropagation();
              onFocus(b.id);
            }}
          >
            <boxGeometry args={[BLOCK_W, height, BLOCK_W]} />
            <meshStandardMaterial
              color={isFocus ? "#f5f5f5" : color}
              emissive={isFocus ? color : "#000"}
              emissiveIntensity={isFocus ? 0.4 : 0}
              roughness={0.5}
              metalness={0.1}
            />
          </mesh>
        );
      })}
    </group>
  );
}

function buildBuildings(nodes: ProjectMapNode[]): Building[] {
  const fileNodes = nodes.filter((n) => n.kind === "file" || n.kind === "module");
  const defCounts = new Map<number | null, number>();
  for (const n of nodes) {
    if (n.kind === "file" || n.kind === "module") continue;
    const key = closestFile(n, fileNodes);
    if (key != null) {
      defCounts.set(key, (defCounts.get(key) ?? 0) + 1);
    }
  }
  return fileNodes.map((n) => ({
    id: n.id,
    name: n.display_name,
    cluster: clusterKeyFromPath(n.path),
    importance: n.importance ?? 0,
    defs: defCounts.get(n.id) ?? 0,
  }));
}

function closestFile(_node: ProjectMapNode, _files: ProjectMapNode[]): number | null {
  return null;
}

function groupByCluster(buildings: Building[]): { cluster: string; buildings: Building[] }[] {
  const map = new Map<string, Building[]>();
  for (const b of buildings) {
    const arr = map.get(b.cluster) ?? [];
    arr.push(b);
    map.set(b.cluster, arr);
  }
  const out = Array.from(map.entries()).map(([cluster, list]) => ({ cluster, buildings: list }));
  out.sort((a, b) => b.buildings.length - a.buildings.length);
  return out;
}

function districtOrigin(idx: number, total: number): [number, number] {
  const cols = Math.max(1, Math.ceil(Math.sqrt(total)));
  const row = Math.floor(idx / cols);
  const col = idx % cols;
  const SPACING = 18;
  const offset = -(cols - 1) * SPACING / 2;
  return [offset + col * SPACING, offset + row * SPACING];
}
