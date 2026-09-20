import { act, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Workspace } from "../../lib/types";
import { WorkspaceItem } from "../Sidebar/WorkspaceItem";
import { EssenceMapCanvasInner } from "./EssenceMapCanvas";
import { extractEssenceSection } from "./essenceSourceSection";
import {
  ESSENCE_MAP_MAX_NODES,
  layoutEssenceMapProject,
  layoutEssenceMapWorld,
  parseEssenceMapJsonl,
  parseEssenceMapProject,
  projectPathsForWorkspace,
  type EssenceMapEdge,
  type EssenceMapFile,
  type EssenceMapNode,
} from "./essenceMapModel";

vi.mock("../../hooks/useAgentProviders", () => ({
  useAgentProviders: () => ({ providers: [], loading: false, error: null, refetch: () => {} }),
}));

vi.mock("../../hooks/useConfig", () => ({
  useConfig: () => ({
    config: { agents: {} },
    loading: false,
    theme: "dark",
    updateConfig: async () => {},
  }),
}));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const workspace: Workspace = {
  id: "ws-map",
  name: "Workspace",
  tabs: ["tab-a", "tab-b"],
  active_tab_id: null,
  cwd: "/repo/a",
};

const source = {
  path: "essences/capabilities.md",
  anchor: "capability:workspaces-and-tabs",
  sha256: "a".repeat(64),
};

const node = (id: string, type: EssenceMapNode["type"], label: string, vector: string | null = null): EssenceMapNode => ({
  kind: "node",
  id,
  type,
  label,
  vector,
  source: { ...source, anchor: id, path: type === "core" || type === "client" ? "essences/core.md" : "essences/capabilities.md" },
  ...(type === "capability" ? { status: "stable" } : {}),
});

const edge = (from: string, to: string, type: EssenceMapEdge["type"]): EssenceMapEdge => ({
  kind: "edge",
  from,
  to,
  type,
  source: { ...source, anchor: from },
});

function mapFile(projectPath: string, records: unknown[]): EssenceMapFile {
  return {
    project_path: projectPath,
    map_path: `${projectPath}/essences/map.jsonl`,
    status: "ready",
    content: records.map((record) => JSON.stringify(record)).join("\n"),
  };
}

const mounted: Array<{ host: HTMLDivElement; root: Root }> = [];

beforeEach(() => {
  document.body.replaceChildren();
});

afterEach(async () => {
  await act(async () => {
    for (const { root } of mounted) root.unmount();
  });
  for (const { host } of mounted) host.remove();
  mounted.length = 0;
});

async function mount(element: ReactNode): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  mounted.push({ host, root });
  await act(async () => root.render(element));
  return { host, root };
}

async function renderMap(
  mapFiles: EssenceMapFile[],
  sourceReader?: (projectPath: string, sourcePath: string) => Promise<string>,
) {
  return mount(
    <EssenceMapCanvasInner
      workspace={workspace}
      mapFiles={mapFiles}
      sourceReader={sourceReader}
    />,
  );
}

async function renderWorkspaceRow(onOpenMap: (workspace: Workspace) => void) {
  return mount(
    <WorkspaceItem
      workspace={workspace}
      isActive
      isExpanded
      onClick={() => {}}
      onToggleExpand={() => {}}
      onClose={() => {}}
      onRename={() => {}}
      onNewTerminal={() => {}}
      onNewSession={() => {}}
      onOpenMap={() => onOpenMap(workspace)}
      onNewAgent={() => {}}
      canClose
      status={null}
      showDisclosure={false}
      detailHeader
    />,
  );
}

async function settle() {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });
}

const validRecords = [
  node("vector:one", "vector", "One vector"),
  node("capability:one", "capability", "One capability"),
  edge("vector:one", "capability:one", "feeds"),
];

const graphVectors = Array.from({ length: 7 }, (_, index) => node(`vector:${index}`, "vector", `Vector ${index}`));
const graphCapabilities = graphVectors.flatMap((vector, vectorIndex) => [
  node(`capability:${vectorIndex}-a`, "capability", `Capability ${vectorIndex} A`, vector.id),
  node(`capability:${vectorIndex}-b`, "capability", `Capability ${vectorIndex} B`, vector.id),
]);
const graphCore = [
  node("core:agents", "core", "Agents and providers"),
  node("core:plans", "core", "Plans and orchestration"),
  node("core:host", "core", "Host state"),
];
const graphClients = [node("client:cli", "client", "CLI"), node("client:ui", "client", "UI")];
const graphNodes = [...graphVectors, ...graphCapabilities, ...graphCore, ...graphClients];
const graphEdges = [
  ...graphCapabilities.map((capability, index) => edge(capability.id, graphCore[index % graphCore.length].id, "runs_on")),
  edge("capability:0-a", "capability:0-b", "feeds"),
  edge("core:agents", "core:plans", "part_of"),
  edge("client:cli", "core:host", "client_of"),
  edge("client:ui", "core:agents", "client_of"),
];

function rectanglesOverlap(
  first: { x: number; y: number; width: number; height: number },
  second: { x: number; y: number; width: number; height: number },
): boolean {
  return first.x < second.x + second.width
    && first.x + first.width > second.x
    && first.y < second.y + second.height
    && first.y + first.height > second.y;
}

describe("map_canvas workspace essence map", () => {
  it("map_canvas_world places project regions deterministically without overlap", () => {
    const projects = [
      parseEssenceMapProject(mapFile("/repo/a", validRecords)),
      parseEssenceMapProject(mapFile("/repo/b", graphNodes)),
      parseEssenceMapProject(mapFile("/repo/c", validRecords)),
    ];
    const world = layoutEssenceMapWorld(projects, 1200);
    const repeat = layoutEssenceMapWorld(projects, 1200);
    expect(world.projects.map(({ file, x, y }) => [file.project_path, x, y])).toEqual(repeat.projects.map(({ file, x, y }) => [file.project_path, x, y]));
    expect(world.projects[0].x).not.toBe(world.projects[1].x);
    expect(world.projects.every((project, index) => world.projects.slice(index + 1).every((other) => project.x + project.layout.width <= other.x || other.x + other.layout.width <= project.x || project.y + project.layout.height <= other.y || other.y + other.layout.height <= project.y))).toBe(true);
    expect(world.projects.every((project) => project.layout.nodes.every((node) => world.projectBounds.has(project.file.project_path) && project.layout.nodeById.has(node.node.id)))).toBe(true);
  });

  it("map_canvas_source turns the anchored row into bullets instead of the whole document", () => {
    const doc = [
      "# CodeTerm capability map",
      "",
      "## 1. Agents live and work in tabs {#vector:agents-live-and-work-in-tabs}",
      "",
      "| ID | Capability | What it enables | Status |",
      "|---|---|---|",
      "| capability:workspaces-and-tabs | Workspaces and tabs | Shells grouped by project | stable |",
      "| capability:agent-providers | Agent providers | Claude and Codex run in a PTY | stable |",
    ].join("\n");
    const section = extractEssenceSection(doc, "capability:agent-providers");
    expect(section).toContain("**Agent providers**");
    expect(section).toContain("- **What it enables:** Claude and Codex run in a PTY");
    expect(section).toContain("- **Status:** stable");
    expect(section).not.toContain("CodeTerm capability map");
    expect(section).not.toContain("Workspaces and tabs");
    expect(section).not.toContain("|");
    expect(extractEssenceSection(doc, "Agent providers")).toBe(doc);
  });

  it("map_canvas_source returns the vector's own heading section and falls back whole", () => {
    const doc = ["# Map", "", "## 1. Agents live in tabs {#vector:agents-live-and-work-in-tabs}", "", "Body A", "", "## 2. Distributed parts {#vector:machines-join-a-mesh}", "", "Body B"].join("\n");
    const section = extractEssenceSection(doc, "vector:machines-join-a-mesh");
    expect(section).toContain("Distributed parts");
    expect(section).toContain("Body B");
    expect(section).not.toContain("Body A");
    expect(extractEssenceSection(doc, "capability:nothing-matches-this")).toBe(doc);
  });

  it("map_canvas_requires stable IDs that match source anchors and graph references", () => {
    const canonical = node("capability:agent-providers", "capability", "Agent providers");
    expect(parseEssenceMapJsonl(JSON.stringify(canonical)).status).toBe("valid");
    expect(parseEssenceMapJsonl(JSON.stringify({ ...canonical, id: "Agent providers", source: { ...canonical.source, anchor: "Agent providers" } })).status).toBe("malformed");
    expect(parseEssenceMapJsonl(JSON.stringify({ ...canonical, source: { ...canonical.source, anchor: "capability:other" } })).status).toBe("malformed");
    expect(parseEssenceMapJsonl(JSON.stringify(edge("Agent providers", "capability:agent-providers", "feeds"))).status).toBe("malformed");
  });

  it("keeps only valid source-backed records", () => {
    const parsed = parseEssenceMapJsonl(validRecords.map((record) => JSON.stringify(record)).join("\n"));
    expect(parsed.status).toBe("valid");
    expect(parsed.nodes).toHaveLength(2);
    expect(parsed.edges).toHaveLength(1);
  });

  it("essence_truth_model preserves lifecycle status and rejects unknown fields", () => {
    const capability = { ...node("capability:planned", "capability", "Planned"), status: "planned" };
    const valid = parseEssenceMapJsonl(JSON.stringify(capability));
    expect(valid.status).toBe("valid");
    const malformed = parseEssenceMapJsonl(JSON.stringify({ ...capability, unexpected: true }));
    expect(malformed.status).toBe("malformed");
  });

  it("keeps a stable layout inside a bounded phone-width cluster", () => {
    const nodes = [
      ...Array.from({ length: 7 }, (_, index) => node(`vector:${index}`, "vector", `Vector ${index}`)),
      node("client:one", "client", "Client"),
    ];
    const layout = layoutEssenceMapProject(nodes, 390);
    const repeat = layoutEssenceMapProject(nodes, 390);
    expect(layout.width).toBeLessThanOrEqual(390);
    expect(layout.nodes.every((item) => item.x >= 0 && item.x + item.width <= layout.width)).toBe(true);
    expect(layout.nodes.map((item) => item.node.id)).toEqual(repeat.nodes.map((item) => item.node.id));
  });

  it("map_canvas_layout groups each capability beside its vector hub", () => {
    const layout = layoutEssenceMapProject(graphNodes, 1180, graphEdges);
    expect(layout.clusters.filter((cluster) => cluster.vector !== null)).toHaveLength(7);
    expect(layout.clusters.every((cluster) => cluster.nodeIds.includes(cluster.vector?.id ?? ""))).toBe(true);
    for (const capability of graphCapabilities) {
      const cluster = layout.clusters.find((candidate) => candidate.nodeIds.includes(capability.id));
      expect(cluster?.vectorId).toBe(capability.vector);
      expect(cluster && cluster.y < layout.nodeById.get(capability.id)!.y).toBe(true);
    }
    expect(layout.regions.map((region) => region.label)).toEqual(["Core parts", "Clients"]);
  });

  it("map_canvas_layout is deterministic, bounded, non-overlapping, and complete", () => {
    const layout = layoutEssenceMapProject(graphNodes, 1180, graphEdges);
    const repeat = layoutEssenceMapProject(graphNodes, 1180, graphEdges);
    expect(layout.nodes.map(({ node, x, y, width, height }) => [node.id, x, y, width, height])).toEqual(
      repeat.nodes.map(({ node, x, y, width, height }) => [node.id, x, y, width, height]),
    );
    expect(layout.nodes.every((item) => item.x >= 0 && item.y >= 0 && item.x + item.width <= layout.width)).toBe(true);
    expect(layout.nodes.every((item, index) => layout.nodes.slice(index + 1).every((other) => !rectanglesOverlap(item, other)))).toBe(true);
    expect(graphEdges).toHaveLength(18);
    expect(graphEdges.every((item) => layout.nodeById.has(item.from) && layout.nodeById.has(item.to))).toBe(true);
  });

  it("map_canvas_layout simplifies phone placement without horizontal overflow", () => {
    const layout = layoutEssenceMapProject(graphNodes, 390, graphEdges);
    const core = layout.regions.find((region) => region.type === "core");
    const clients = layout.regions.find((region) => region.type === "client");
    expect(layout.width).toBeLessThanOrEqual(390);
    expect(layout.nodes.every((item) => item.x >= 0 && item.x + item.width <= layout.width)).toBe(true);
    expect(layout.nodes.every((item, index) => layout.nodes.slice(index + 1).every((other) => !rectanglesOverlap(item, other)))).toBe(true);
    expect(new Set(layout.nodes.filter((item) => item.node.type === "vector").map((item) => item.x)).size).toBe(1);
    expect(clients && core && clients.y > core.y + core.height).toBe(true);
  });

  it("collects each workspace project tree once", () => {
    const paths = projectPathsForWorkspace(workspace, {
      "tab-a": { cwd: "/repo/a" },
      "tab-b": { cwd: "/repo/b" },
    });
    expect(paths).toEqual(["/repo/a", "/repo/b"]);
  });

  it("renders multi-project clusters with accessible node names and no horizontal overflow", async () => {
    const { host } = await renderMap([
      mapFile("/repo/a", validRecords),
      mapFile("/repo/b", [node("core:one", "core", "One core")]),
    ]);
    expect(host.querySelector('[data-testid="essence-map-canvas"]')).not.toBeNull();
    expect(host.querySelectorAll('[data-testid="essence-map-project"]')).toHaveLength(2);
    const buttons = host.querySelectorAll<HTMLButtonElement>('[data-testid="essence-map-node"]');
    expect(buttons).toHaveLength(3);
    expect(buttons[0].getAttribute("aria-label")).toBe("One vector (vector)");
    expect(host.querySelectorAll('[data-testid="essence-map-edge"]')).toHaveLength(1);
    expect(host.querySelector('[data-testid="essence-map-edge"]')?.getAttribute("d")).toContain("C");
    expect(buttons[0].getAttribute("data-node-type")).toBe("vector");
    expect(host.querySelector('[data-testid="essence-map-projects"]')?.className).toContain("overflow-hidden");
    expect(host.querySelector('[data-testid="essence-map-project-world"]')?.getAttribute("style")).toContain("height");
  });

  it("exposes the workspace map action beside the menu and activates its callback", async () => {
    const onOpenMap = vi.fn();
    const { host } = await renderWorkspaceRow(onOpenMap);
    const action = host.querySelector<HTMLButtonElement>(
      'button[aria-label="Open essence map for Workspace"]',
    );
    expect(action).not.toBeNull();
    expect(action?.getAttribute("type")).toBe("button");
    expect(action?.parentElement?.querySelector('button[aria-label="Actions for Workspace"]')).not.toBeNull();
    await act(async () => action?.click());
    expect(onOpenMap).toHaveBeenCalledWith(workspace);
  });

  it("opens the exact referenced essence in an accessible side panel", async () => {
    const exactSource = [
      "# CodeTerm capability map",
      "",
      "## 1. Agents live and work in tabs {#vector:agents-live-and-work-in-tabs}",
      "",
      "| ID | Capability | What it enables | Status |",
      "|---|---|---|---|",
      "| capability:agent-providers | Agent providers | Exact stable source row | stable |",
      "| capability:other | Other capability | Must stay hidden | stable |",
    ].join("\n");
    const sourceReader = vi.fn(async (projectPath: string, sourcePath: string) => {
      expect(projectPath).toBe("/repo/a");
      expect(sourcePath).toBe("essences/capabilities.md");
      return exactSource;
    });
    const capability = node("capability:agent-providers", "capability", "Agent providers");
    const { host } = await renderMap([mapFile("/repo/a", [capability])], sourceReader);
    const button = host.querySelector<HTMLButtonElement>('[data-node-id="capability:agent-providers"]');
    expect(button).not.toBeNull();
    await act(async () => button?.click());
    await settle();
    const panel = host.querySelector('[data-testid="essence-source-panel"]');
    expect(panel?.getAttribute("role")).toBe("dialog");
    expect(panel?.textContent).toContain("Exact stable source row");
    expect(panel?.textContent).not.toContain("Must stay hidden");
    expect(sourceReader).toHaveBeenCalledTimes(1);
  });

  it("gives explicit empty malformed oversized and unreadable states without nodes", async () => {
    const oversized = Array.from({ length: ESSENCE_MAP_MAX_NODES + 1 }, (_, index) =>
      node(`capability:${index}`, "capability", `Capability ${index}`));
    const { host } = await renderMap([
      { project_path: "/repo/empty", map_path: "/repo/empty/essences/map.jsonl", status: "empty", content: "" },
      mapFile("/repo/malformed", ["not an object"]),
      mapFile("/repo/oversized", oversized),
      {
        project_path: "/repo/unreadable",
        map_path: "/repo/unreadable/essences/map.jsonl",
        status: "unreadable",
        error: "permission denied",
      },
    ]);
    await settle();
    expect(host.querySelector('[data-testid="essence-map-project-empty"]')).not.toBeNull();
    expect(host.querySelector('[data-testid="essence-map-project-malformed"]')).not.toBeNull();
    expect(host.querySelector('[data-testid="essence-map-project-oversized"]')).not.toBeNull();
    expect(host.querySelector('[data-testid="essence-map-project-unreadable"]')).not.toBeNull();
    expect(host.querySelectorAll('[data-testid="essence-map-node"]')).toHaveLength(0);
  });
});
