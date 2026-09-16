import { act, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Workspace } from "../../lib/types";
import { WorkspaceItem } from "../Sidebar/WorkspaceItem";
import { EssenceMapCanvasInner } from "./EssenceMapCanvas";
import {
  ESSENCE_MAP_MAX_NODES,
  layoutEssenceMapProject,
  parseEssenceMapJsonl,
  projectPathsForWorkspace,
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
  path: "essences/CAPABILITIES.md",
  anchor: "capability:workspaces-and-tabs",
  sha256: "a".repeat(64),
};

const node = (id: string, type: EssenceMapNode["type"], label: string): EssenceMapNode => ({
  kind: "node",
  id,
  type,
  label,
  vector: null,
  source,
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
  {
    kind: "edge",
    from: "vector:one",
    to: "capability:one",
    type: "feeds",
    source,
  },
];

describe("map_canvas workspace essence map", () => {
  it("keeps only valid source-backed records", () => {
    const parsed = parseEssenceMapJsonl(validRecords.map((record) => JSON.stringify(record)).join("\n"));
    expect(parsed.status).toBe("valid");
    expect(parsed.nodes).toHaveLength(2);
    expect(parsed.edges).toHaveLength(1);
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
    expect(host.querySelector('[data-testid="essence-map-projects"]')?.className).toContain("overflow-x-hidden");
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
    const exactSource = "# exact referenced essence\ncapability:workspaces-and-tabs";
    const sourceReader = vi.fn(async (projectPath: string, sourcePath: string) => {
      expect(projectPath).toBe("/repo/a");
      expect(sourcePath).toBe("essences/CAPABILITIES.md");
      return exactSource;
    });
    const { host } = await renderMap([mapFile("/repo/a", validRecords)], sourceReader);
    const button = host.querySelector<HTMLButtonElement>('[data-node-id="capability:one"]');
    expect(button).not.toBeNull();
    await act(async () => button?.click());
    await settle();
    const panel = host.querySelector('[data-testid="essence-source-panel"]');
    expect(panel?.getAttribute("role")).toBe("dialog");
    expect(panel?.querySelector("pre")?.textContent).toBe(exactSource);
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
