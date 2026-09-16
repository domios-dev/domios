import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import type { Workspace } from "../../lib/types";
import { EssenceMapCanvasInner } from "./EssenceMapCanvas";
import {
  ESSENCE_MAP_MAX_NODES,
  layoutEssenceMapProject,
  parseEssenceMapJsonl,
  projectPathsForWorkspace,
  type EssenceMapFile,
  type EssenceMapNode,
} from "./essenceMapModel";

type TestFn = () => void | Promise<void>;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

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

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/essence-map" });
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
  Object.defineProperty(globalThis, "HTMLButtonElement", { value: window.HTMLButtonElement, configurable: true });
  Object.defineProperty(globalThis, "ResizeObserver", {
    configurable: true,
    value: class {
      observe() {}
      disconnect() {}
    },
  });
}

async function render(
  mapFiles: EssenceMapFile[],
  sourceReader?: (projectPath: string, sourcePath: string) => Promise<string>,
): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(
      <EssenceMapCanvasInner
        workspace={workspace}
        mapFiles={mapFiles}
        sourceReader={sourceReader}
      />,
    );
  });
  return { host, root };
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

test("map_canvas parser keeps only valid source-backed records", () => {
  const parsed = parseEssenceMapJsonl(validRecords.map((record) => JSON.stringify(record)).join("\n"));
  assert(parsed.status === "valid", "valid map parses");
  assert(parsed.nodes.length === 2, "only the two source-backed nodes are rendered");
  assert(parsed.edges.length === 1, "the source-backed edge is retained");
});

test("map_canvas layout stays inside its bounded phone-width cluster", () => {
  const layout = layoutEssenceMapProject(
    [
      ...Array.from({ length: 7 }, (_, index) => node(`vector:${index}`, "vector", `Vector ${index}`)),
      node("client:one", "client", "Client"),
    ],
    390,
  );
  const repeat = layoutEssenceMapProject(
    [
      ...Array.from({ length: 7 }, (_, index) => node(`vector:${index}`, "vector", `Vector ${index}`)),
      node("client:one", "client", "Client"),
    ],
    390,
  );
  assert(layout.width <= 390, `layout width is bounded (${layout.width})`);
  assert(layout.nodes.every((item) => item.x >= 0 && item.x + item.width <= layout.width), "every node fits inside the layout");
  assert(layout.nodes.map((item) => item.node.id).join(",") === repeat.nodes.map((item) => item.node.id).join(","), "layout order is stable");
});

test("map_canvas collects each workspace project tree once", () => {
  const paths = projectPathsForWorkspace(workspace, {
    "tab-a": { cwd: "/repo/a" },
    "tab-b": { cwd: "/repo/b" },
  });
  assert(paths.join(",") === "/repo/a,/repo/b", `project roots are deduplicated: ${paths.join(",")}`);
});

test("map_canvas renders multi-project clusters with accessible node names", async () => {
  const { host, root } = await render([
    mapFile("/repo/a", validRecords),
    mapFile("/repo/b", [node("core:one", "core", "One core")]),
  ]);
  assert(host.querySelector('[data-testid="essence-map-canvas"]'), "map canvas is mounted");
  assert(host.querySelectorAll('[data-testid="essence-map-project"]').length === 2, "both project clusters render");
  const buttons = host.querySelectorAll<HTMLButtonElement>('[data-testid="essence-map-node"]');
  assert(buttons.length === 3, `rendered node count comes from maps (${buttons.length})`);
  assert(buttons[0].getAttribute("aria-label") === "One vector (vector)", "node has an accessible source label");
  assert(host.querySelector('[data-testid="essence-map-projects"]')?.className.includes("overflow-x-hidden"), "projects hide horizontal overflow");
  await act(async () => root.unmount());
});

test("map_canvas node activation opens the exact referenced essence in a side panel", async () => {
  const calls: string[] = [];
  const { host, root } = await render(
    [mapFile("/repo/a", validRecords)],
    async (projectPath, sourcePath) => {
      calls.push(`${projectPath}:${sourcePath}`);
      return "# exact referenced essence\ncapability:workspaces-and-tabs";
    },
  );
  const button = host.querySelector<HTMLButtonElement>('[data-node-id="capability:one"]');
  assert(button, "capability node is activatable");
  await act(async () => {
    button.click();
    await settle();
  });
  const panel = host.querySelector('[data-testid="essence-source-panel"]');
  assert(panel?.getAttribute("role") === "dialog", "source opens in an accessible dialog");
  assert(panel?.textContent?.includes("# exact referenced essence"), "panel shows exact source content");
  assert(calls.join(",") === "/repo/a:essences/CAPABILITIES.md", "activation reads the node source reference");
  await act(async () => root.unmount());
});

test("map_canvas gives explicit empty malformed and oversized states without nodes", async () => {
  const oversized = Array.from({ length: ESSENCE_MAP_MAX_NODES + 1 }, (_, index) =>
    node(`capability:${index}`, "capability", `Capability ${index}`));
  const { host, root } = await render([
    { project_path: "/repo/empty", map_path: "/repo/empty/essences/map.jsonl", status: "empty", content: "" },
    mapFile("/repo/malformed", ["not an object"]),
    mapFile("/repo/oversized", oversized),
  ]);
  await settle();
  assert(host.querySelector('[data-testid="essence-map-project-empty"]'), "empty map state is explicit");
  assert(host.querySelector('[data-testid="essence-map-project-malformed"]'), "malformed map state is explicit");
  assert(host.querySelector('[data-testid="essence-map-project-oversized"]'), "oversized map state is explicit");
  assert(host.querySelectorAll('[data-testid="essence-map-node"]').length === 0, "invalid maps cannot invent nodes");
  await act(async () => root.unmount());
});

async function main() {
  setupDom();
  let passed = 0;
  for (const [name, fn] of tests) {
    try {
      await fn();
      passed++;
      console.log(`  ✓ ${name}`);
    } catch (error) {
      console.error(`  ✗ ${name}`);
      console.error(error instanceof Error ? error.stack : error);
      process.exit(1);
    }
  }
  console.log(`\n✓ ${passed}/${tests.length} map_canvas tests passed`);
}

main();
