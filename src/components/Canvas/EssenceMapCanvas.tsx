import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { AlertTriangle, FileText, Map as MapIcon, RefreshCw, X } from "lucide-react";
import { useMuxState } from "../../store/multiplexer";
import type { Tab, Workspace } from "../../lib/types";
import { UI_PANEL, UI_TEXT } from "../shared/design";
import { fetchEssenceSource, fetchWorkspaceEssenceMaps } from "./essenceMapApi";
import {
  layoutEssenceMapProject,
  parseEssenceMapProject,
  projectPathsForWorkspace,
  type EssenceMapFile,
  type EssenceMapNode,
  type EssenceMapNodeLayout,
  type EssenceMapProject,
} from "./essenceMapModel";

interface EssenceMapCanvasProps {
  workspaceId: string;
  onClose: () => void;
}

interface EssenceMapCanvasInnerProps {
  workspace: Workspace;
  mapFiles: EssenceMapFile[];
  loading?: boolean;
  error?: string | null;
  onRetry?: () => void;
  onClose?: () => void;
  sourceReader?: (projectPath: string, sourcePath: string) => Promise<string>;
}

interface SourcePanelState {
  node: EssenceMapNode;
  state: "loading" | "ready" | "error";
  content?: string;
  error?: string;
}

const DEFAULT_CANVAS_WIDTH = 720;

function projectName(projectPath: string): string {
  const normalized = projectPath.replaceAll("\\", "/").replace(/\/+$/, "");
  return normalized.split("/").pop() || "Project";
}

function projectStatusLabel(project: EssenceMapProject): string {
  switch (project.status) {
    case "valid": return `${project.nodes.length} nodes · ${project.edges.length} links`;
    case "empty": return "Empty essence map";
    case "malformed": return "Malformed essence map";
    case "oversized": return "Essence map is oversized";
    case "missing": return "No essences/map.jsonl found";
    case "unreadable": return "Essence map could not be read";
  }
}

function edgePath(source: EssenceMapNodeLayout, target: EssenceMapNodeLayout): string {
  const x1 = source.x + source.width / 2;
  const y1 = source.y + source.height;
  const x2 = target.x + target.width / 2;
  const y2 = target.y;
  const middle = y1 + (y2 - y1) / 2;
  return `M ${x1} ${y1} C ${x1} ${middle}, ${x2} ${middle}, ${x2} ${y2}`;
}

function projectHasContent(projects: EssenceMapProject[]): boolean {
  return projects.some((project) => project.status === "valid" && project.nodes.length > 0);
}

function EssenceMapProjectView({
  project,
  width,
  onActivate,
}: {
  project: EssenceMapProject;
  width: number;
  onActivate: (project: EssenceMapProject, node: EssenceMapNode) => void;
}) {
  const layout = useMemo(
    () => layoutEssenceMapProject(project.nodes, width),
    [project.nodes, width],
  );
  const nodeById = layout.nodeById;
  return (
    <section
      className="ct-panel-card relative min-w-0 overflow-hidden"
      data-testid="essence-map-project"
      data-project-path={project.file.project_path}
      aria-labelledby={`essence-project-${project.file.project_path}`}
    >
      <div className="flex min-w-0 items-start justify-between gap-3 border-b border-[var(--ct-border)] px-4 py-3">
        <div className="min-w-0">
          <h2 id={`essence-project-${project.file.project_path}`} className="truncate text-sm font-semibold text-[var(--ct-fg)]">
            {projectName(project.file.project_path)}
          </h2>
          <p className={`${UI_TEXT.detailMuted} truncate`} title={project.file.project_path}>
            {project.file.project_path}
          </p>
        </div>
        <span className={`${UI_TEXT.metaMuted} shrink-0`}>
          {projectStatusLabel(project)}
        </span>
      </div>
      {project.status === "valid" && project.nodes.length > 0 ? (
        <div
          className="relative max-w-full overflow-hidden px-0"
          data-testid="essence-map-project-world"
          style={{ height: layout.height }}
        >
          <svg
            aria-hidden="true"
            className="pointer-events-none absolute inset-0 h-full w-full"
            viewBox={`0 0 ${layout.width} ${layout.height}`}
            preserveAspectRatio="none"
          >
            {project.edges.map((edge) => {
              const source = nodeById.get(edge.from);
              const target = nodeById.get(edge.to);
              if (!source || !target) return null;
              return (
                <path
                  key={`${edge.type}:${edge.from}:${edge.to}`}
                  d={edgePath(source, target)}
                  fill="none"
                  stroke="var(--ct-border)"
                  strokeWidth="1.25"
                  opacity="0.72"
                />
              );
            })}
          </svg>
          {layout.nodes.map((item) => (
            <button
              key={item.node.id}
              type="button"
              data-testid="essence-map-node"
              data-node-id={item.node.id}
              aria-label={`${item.node.label} (${item.node.type})`}
              className="absolute overflow-hidden rounded-md border border-[var(--ct-border)] bg-[var(--ct-surface)] px-3 py-2 text-left shadow-sm transition-colors hover:border-[var(--ct-accent)] hover:bg-[var(--ct-hover)] focus-visible:z-10 focus-visible:outline focus-visible:outline-2 focus-visible:outline-[var(--ct-accent)]"
              style={{ left: item.x, top: item.y, width: item.width, height: item.height }}
              onClick={() => onActivate(project, item.node)}
            >
              <span className="block text-[9px] uppercase tracking-[0.16em] text-[var(--ct-muted)]">
                {item.node.type}
              </span>
              <span className="mt-1 block max-h-10 overflow-hidden text-[11px] font-medium leading-4 text-[var(--ct-fg)]">
                {item.node.label}
              </span>
              <span className="sr-only">Source {item.node.source.path}, anchor {item.node.source.anchor}</span>
            </button>
          ))}
        </div>
      ) : (
        <div
          className="flex min-h-24 items-center gap-3 px-4 py-5 text-xs text-[var(--ct-muted-fg)]"
          data-testid={`essence-map-project-${project.status}`}
        >
          {project.status === "malformed" || project.status === "oversized" || project.status === "unreadable" ? (
            <AlertTriangle size={15} aria-hidden="true" className="shrink-0 text-[var(--ct-yellow)]" />
          ) : (
            <FileText size={15} aria-hidden="true" className="shrink-0" />
          )}
          <span>{project.error ?? projectStatusLabel(project)}</span>
        </div>
      )}
    </section>
  );
}

function EssenceSourcePanel({
  source,
  onClose,
}: {
  source: SourcePanelState;
  onClose: () => void;
}) {
  const panelRef = useRef<HTMLElement>(null);
  useLayoutEffect(() => {
    panelRef.current?.focus();
  }, []);
  return (
    <aside
      ref={panelRef}
      className="absolute inset-y-0 right-0 z-30 flex w-[min(36rem,100%)] min-w-0 flex-col border-l border-[var(--ct-border)] bg-[var(--ct-titlebar-bg)] p-4 shadow-2xl outline-none"
      data-testid="essence-source-panel"
      role="dialog"
      aria-modal="false"
      aria-labelledby="essence-source-panel-title"
      tabIndex={-1}
    >
      <div className="flex min-w-0 items-start justify-between gap-3 border-b border-[var(--ct-border)] pb-3">
        <div className="min-w-0">
          <p className={UI_TEXT.sectionLabel}>Referenced essence</p>
          <h2 id="essence-source-panel-title" className="truncate text-sm font-semibold text-[var(--ct-fg)]">
            {source.node.label}
          </h2>
          <p className={`${UI_TEXT.detailMuted} truncate`} title={source.node.source.path}>
            {source.node.source.path} · {source.node.source.anchor}
          </p>
        </div>
        <button
          type="button"
          className={UI_PANEL.drawerClose}
          aria-label="Close referenced essence"
          title="Close referenced essence"
          onClick={onClose}
        >
          <X size={14} aria-hidden="true" />
        </button>
      </div>
      <div className="min-h-0 flex-1 overflow-auto pt-3">
        {source.state === "loading" && (
          <div className="flex items-center gap-2 text-xs text-[var(--ct-muted-fg)]" role="status" aria-live="polite">
            <RefreshCw size={13} className="animate-spin" aria-hidden="true" />
            Reading referenced essence…
          </div>
        )}
        {source.state === "error" && (
          <p className="text-xs text-[var(--ct-red)]" role="alert">
            {source.error ?? "Referenced essence could not be read."}
          </p>
        )}
        {source.state === "ready" && (
          <pre className="max-w-full whitespace-pre-wrap break-words font-mono text-[11px] leading-5 text-[var(--ct-fg)]">
            {source.content}
          </pre>
        )}
      </div>
    </aside>
  );
}

export function EssenceMapCanvasInner({
  workspace,
  mapFiles,
  loading = false,
  error = null,
  onRetry,
  onClose,
  sourceReader = fetchEssenceSource,
}: EssenceMapCanvasInnerProps) {
  const viewportRef = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(DEFAULT_CANVAS_WIDTH);
  const [source, setSource] = useState<SourcePanelState | null>(null);
  const sourceRequest = useRef(0);
  const projects = useMemo(() => mapFiles.map(parseEssenceMapProject), [mapFiles]);
  const hasContent = projectHasContent(projects);

  useLayoutEffect(() => {
    const viewport = viewportRef.current;
    if (!viewport) return;
    const update = () => {
      if (viewport.clientWidth > 0) setWidth(Math.max(280, viewport.clientWidth - 32));
    };
    update();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(update);
    observer.observe(viewport);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    if (!source) return;
    const exists = projects.some((project) => project.nodes.some((node) => node.id === source.node.id));
    if (!exists) setSource(null);
  }, [projects, source]);

  const closeSource = useCallback(() => setSource(null), []);
  const activateNode = useCallback((project: EssenceMapProject, node: EssenceMapNode) => {
    const request = sourceRequest.current + 1;
    sourceRequest.current = request;
    setSource({ node, state: "loading" });
    void sourceReader(project.file.project_path, node.source.path).then(
      (content) => {
        if (sourceRequest.current === request) setSource({ node, state: "ready", content });
      },
      (reason) => {
        if (sourceRequest.current === request) {
          setSource({
            node,
            state: "error",
            error: reason instanceof Error ? reason.message : String(reason),
          });
        }
      },
    );
  }, [sourceReader]);

  return (
    <div
      className="absolute inset-0 flex min-w-0 flex-col overflow-hidden bg-[var(--ct-bg)] pt-[var(--ct-safe-top,0px)] pb-[var(--ct-safe-bottom,0px)]"
      data-testid="essence-map-canvas"
      role="region"
      aria-labelledby="essence-map-title"
    >
      <header className="flex min-h-12 min-w-0 shrink-0 items-center gap-3 border-b border-[var(--ct-border)] bg-[var(--ct-titlebar-bg)] px-4 py-2 text-[var(--ct-fg)]">
        <MapIcon size={17} aria-hidden="true" className="shrink-0 text-[var(--ct-accent)]" />
        <div className="min-w-0 flex-1">
          <h1 id="essence-map-title" className="truncate text-sm font-semibold">Workspace essence map</h1>
          <p className={`${UI_TEXT.detailMuted} truncate`}>{workspace.name}</p>
        </div>
        <span className={`${UI_TEXT.metaMuted} hidden shrink-0 sm:inline`} aria-live="polite">
          {loading ? "Reading maps…" : hasContent ? `${projects.filter((project) => project.status === "valid").length} project maps` : "No rendered nodes"}
        </span>
        {onClose && (
          <button
            type="button"
            className={UI_PANEL.drawerClose}
            data-testid="essence-map-close"
            aria-label="Close workspace essence map"
            title="Close workspace essence map"
            onClick={onClose}
          >
            <X size={15} aria-hidden="true" />
          </button>
        )}
      </header>
      <div ref={viewportRef} className="relative min-h-0 min-w-0 flex-1 overflow-x-hidden overflow-y-auto" data-testid="essence-map-projects" aria-label="Essence map projects">
        {loading && (
          <div className="flex h-full items-center justify-center p-6 text-xs text-[var(--ct-muted-fg)]" role="status" aria-live="polite">
            Reading project essence maps…
          </div>
        )}
        {!loading && error && (
          <div className="flex h-full flex-col items-center justify-center gap-3 p-6 text-center text-xs text-[var(--ct-red)]" role="alert">
            <AlertTriangle size={18} aria-hidden="true" />
            <p>{error}</p>
            {onRetry && (
              <button type="button" className={UI_PANEL.drawerClose} onClick={onRetry}>
                Try again
              </button>
            )}
          </div>
        )}
        {!loading && !error && projects.length === 0 && (
          <div className="flex h-full items-center justify-center p-6 text-center text-xs text-[var(--ct-muted-fg)]" data-testid="essence-map-empty">
            No project trees are attached to this workspace.
          </div>
        )}
        {!loading && !error && projects.length > 0 && !hasContent && (
          <div className="mx-auto max-w-xl px-5 pb-1 pt-5 text-center text-xs text-[var(--ct-muted-fg)]" data-testid="essence-map-no-nodes">
            No source-backed essence nodes are available. Empty, malformed, oversized, or missing maps are shown below without inventing nodes.
          </div>
        )}
        {!loading && !error && projects.length > 0 && (
          <div className="mx-auto flex w-full max-w-[1200px] min-w-0 flex-col gap-4 p-4">
            {projects.map((project) => (
              <EssenceMapProjectView
                key={project.file.project_path}
                project={project}
                width={width}
                onActivate={activateNode}
              />
            ))}
          </div>
        )}
        {source && <EssenceSourcePanel source={source} onClose={closeSource} />}
      </div>
    </div>
  );
}

function useWorkspaceEssenceMapData(workspace: Workspace | undefined, tabs: Record<string, Tab>) {
  const projectPaths = useMemo(
    () => workspace ? projectPathsForWorkspace(workspace, tabs) : [],
    [tabs, workspace],
  );
  const [state, setState] = useState<{
    status: "loading" | "ready" | "error";
    files: EssenceMapFile[];
    error: string | null;
  }>({ status: "loading", files: [], error: null });

  useEffect(() => {
    let active = true;
    if (projectPaths.length === 0) {
      setState({ status: "ready", files: [], error: null });
      return () => { active = false; };
    }
    setState((current) => ({ ...current, status: "loading", error: null }));
    void fetchWorkspaceEssenceMaps(projectPaths).then(
      (files) => { if (active) setState({ status: "ready", files, error: null }); },
      (reason) => { if (active) setState({ status: "error", files: [], error: reason instanceof Error ? reason.message : String(reason) }); },
    );
    return () => { active = false; };
  }, [projectPaths]);

  const retry = useCallback(() => {
    if (projectPaths.length === 0) return;
    setState((current) => ({ ...current, status: "loading", error: null }));
    void fetchWorkspaceEssenceMaps(projectPaths).then(
      (files) => setState({ status: "ready", files, error: null }),
      (reason) => setState({ status: "error", files: [], error: reason instanceof Error ? reason.message : String(reason) }),
    );
  }, [projectPaths]);
  return { ...state, retry };
}

export function EssenceMapCanvas({ workspaceId, onClose }: EssenceMapCanvasProps) {
  const state = useMuxState();
  const workspace = state.workspaces.find((candidate) => candidate.id === workspaceId);
  const { status, files, error, retry } = useWorkspaceEssenceMapData(workspace, state.tabs);
  useEffect(() => {
    if (!workspace) onClose();
  }, [onClose, workspace]);
  if (!workspace) return null;
  return (
    <EssenceMapCanvasInner
      workspace={workspace}
      mapFiles={files}
      loading={status === "loading"}
      error={error}
      onRetry={retry}
      onClose={onClose}
      sourceReader={fetchEssenceSource}
    />
  );
}
