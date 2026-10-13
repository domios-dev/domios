import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { AlertTriangle, List, Network, RefreshCw, X } from "lucide-react";
import { CanvasToolbar } from "./CanvasToolbar";
import { BusySpinner } from "../shared/BusyState";
import { useMultiplexer, useMuxState } from "../../store/multiplexer";
import type { ProviderDescriptor, Tab, Workspace } from "../../lib/types";
import { useAgentProviders } from "../../hooks/useAgentProviders";
import { CartographerMapDialog } from "./CartographerMapDialog";
import { cartographerMapActions, startCartographerMap } from "./cartographerMapAction";
import type { CartographerMapDraft, CartographerMapMode } from "./cartographerMapModel";
import { UI_PANEL, UI_TEXT } from "../shared/design";
import { MarkdownDescription } from "../Extensions/MarkdownDescription";
import { nativeBridge } from "../../lib/nativeBridge";
import { fetchEssenceSource, fetchWorkspaceEssenceMaps } from "./essenceMapApi";
import { extractEssenceSection } from "./essenceSourceSection";
import { EssenceMapProjectView, projectName } from "./EssenceMapProjectView";
import { layoutEssenceMapWorld, parseEssenceMapProject, projectPathsForWorkspace, type EssenceMapFile, type EssenceMapNode, type EssenceMapProject } from "./essenceMapModel";
import { formatLayerTransform, useCanvasViewport } from "./useCanvasViewport";
import { EssenceDocumentTree, type EssenceDocumentEntry } from "./essenceDocumentTree";
import { EssenceMapEmptyState } from "./EssenceMapEmptyState";
import { askOrchestratorToBuildMap, findWorkspaceOrchestrator, type MapOrchestrator } from "./essenceMapOrchestrator";

interface EssenceMapCanvasProps { workspaceId: string; onClose: () => void; }
interface EssenceMapCanvasInnerProps { workspace: Workspace; mapFiles: EssenceMapFile[]; loading?: boolean; error?: string | null; onRetry?: () => void; sourceReader?: (projectPath: string, sourcePath: string) => Promise<string>; visible?: boolean; documentSelection?: { project: EssenceMapProject; entry: EssenceDocumentEntry } | null; orchestrator?: MapOrchestrator | null; onAskOrchestrator?: (orchestrator: MapOrchestrator) => Promise<void>; providers?: ProviderDescriptor[]; providersLoading?: boolean; providersError?: string | null; onStartCartographer?: (draft: CartographerMapDraft) => Promise<void>; onOpenCartographer?: (mode: CartographerMapMode) => void; toolbarView?: ReactNode; }
interface SourcePanelState { node: EssenceMapNode; projectPath: string; state: "loading" | "ready" | "error"; content?: string; error?: string; }
const DEFAULT_WIDTH = 720;
const ZOOM_STEP = 0.25;

/** Windows extended-length paths carry a \\?\ prefix that must never reach the UI. */
function displayPath(path: string): string { return path.replace(/^\\\\\?\\/, ""); }

function EssenceSourcePanel({ source, onClose }: { source: SourcePanelState; onClose: () => void }) {
  const panelRef = useRef<HTMLElement>(null);
  useLayoutEffect(() => { panelRef.current?.focus(); }, []);
  return <aside ref={panelRef} className="absolute inset-y-0 right-0 z-30 flex w-[min(36rem,100%)] min-w-0 flex-col bg-[var(--ct-bg)] p-4 shadow-[var(--ct-shadow-capsule)] outline-none" data-testid="essence-source-panel" role="dialog" aria-labelledby="essence-source-panel-title" tabIndex={-1}><div className="ct-jobs-header flex min-w-0 items-start justify-between gap-3 pb-3"><div className="min-w-0"><p className={UI_TEXT.sectionLabel}>Referenced essence</p><h2 id="essence-source-panel-title" className="truncate text-sm font-semibold text-[var(--ct-fg)]">{source.node.label}</h2><button type="button" data-testid="essence-source-open" className={`${UI_TEXT.detailMuted} block max-w-full truncate text-left hover:text-[var(--ct-accent)] hover:underline focus-visible:outline focus-visible:outline-2 focus-visible:outline-[var(--ct-accent)]`} title={`Open ${displayPath(source.projectPath)}/${source.node.source.path}`} onClick={() => { void nativeBridge.openFolder(`${displayPath(source.projectPath)}/${source.node.source.path}`); }}>{displayPath(source.projectPath)} · {source.node.source.path} · {source.node.source.anchor}</button><p className="sr-only">Owner vector or essence: {source.node.vector ?? source.node.id}</p></div><button type="button" className={UI_PANEL.drawerClose} aria-label="Close referenced essence" onClick={onClose}><X size={14} aria-hidden="true" /></button></div><div className="min-h-0 flex-1 overflow-auto pt-3">{source.state === "loading" && <div className="flex items-center gap-2 text-xs text-[var(--ct-muted-fg)]" role="status"><BusySpinner icon={RefreshCw} size={13} />Reading referenced essence…</div>}{source.state === "error" && <p className="text-xs text-[var(--ct-red)]" role="alert">{source.error ?? "Referenced essence could not be read."}</p>}{source.state === "ready" && <MarkdownDescription markdown={extractEssenceSection(source.content ?? "", source.node.source.anchor)} />}</div></aside>;
}


function EssenceMapCanvasInnerBase({ workspace, mapFiles, loading = false, error = null, onRetry, sourceReader = fetchEssenceSource, visible = true, documentSelection = null, orchestrator = null, onAskOrchestrator, onOpenCartographer, toolbarView }: EssenceMapCanvasInnerProps) {
  const [size, setSize] = useState({ width: DEFAULT_WIDTH, height: 600 });
  const [source, setSource] = useState<SourcePanelState | null>(null);
  const requestRef = useRef(0);
  const projects = useMemo(() => mapFiles.map(parseEssenceMapProject), [mapFiles]);
  const world = useMemo(() => layoutEssenceMapWorld(projects, size.width), [projects, size.width]);
  const hasContent = projects.some((project) => project.status === "valid" && project.nodes.length > 0);
  const viewport = useCanvasViewport({ width: size.width, height: size.height, minScale: 0.2, maxScale: 2.5, fitPadding: 48, ignoreSelector: "[data-graph-toolbar], [data-essence-map-control]", wheelBehavior: "zoom" });
  const { fitToView, animateToView, zoomBy } = viewport;
  const fitAll = useCallback(() => fitToView({ x: 0, y: 0, width: world.width, height: world.height }), [fitToView, world.width, world.height]);
  useLayoutEffect(() => { const element = viewport.containerRef.current; if (!element) return; const update = () => { if (element.clientWidth > 0 && element.clientHeight > 0) setSize({ width: element.clientWidth, height: element.clientHeight }); }; update(); if (typeof ResizeObserver === "undefined") return; const observer = new ResizeObserver(update); observer.observe(element); return () => observer.disconnect(); }, [viewport.containerRef]);
  // Fit once per map and viewport size. Keying the effect on fitAll re-fit on every
  // render, which reset the camera a frame after each zoom or pan.
  const fittedKey = useRef<string | null>(null);
  useEffect(() => {
    if (world.projects.length === 0 || size.width <= 0 || size.height <= 0) return;
    // Keyed on the maps alone, never the size: the view mounts at a default width,
    // so a size-keyed fit would re-fire after the viewer already zoomed or panned.
    const key = world.projects.map((project) => project.file.project_path).join("|");
    if (fittedKey.current === key) return;
    fittedKey.current = key;
    fitAll();
  }, [fitAll, size.height, size.width, world.projects]);
  useEffect(() => {
    if (!source) return;
    const mapNodeStillExists = projects.some((project) => project.nodes.some((node) => node.id === source.node.id));
    const documentStillExists = projects.some((project) => project.file.project_path === source.projectPath && project.file.documents?.includes(source.node.source.path));
    if (!mapNodeStillExists && !documentStillExists) setSource(null);
  }, [projects, source]);
  const flyTo = useCallback((path: string) => { const bounds = world.projectBounds.get(path); if (bounds) animateToView(bounds); }, [animateToView, world.projectBounds]);
  const activateNode = useCallback((project: EssenceMapProject, node: EssenceMapNode) => { const request = requestRef.current + 1; requestRef.current = request; setSource({ node, projectPath: project.file.project_path, state: "loading" }); void sourceReader(project.file.project_path, node.source.path).then((content) => { if (requestRef.current === request) setSource({ node, projectPath: project.file.project_path, state: "ready", content }); }, (reason) => { if (requestRef.current === request) setSource({ node, projectPath: project.file.project_path, state: "error", error: reason instanceof Error ? reason.message : String(reason) }); }); }, [sourceReader]);
  useEffect(() => { if (!visible && documentSelection) activateNode(documentSelection.project, documentSelection.entry.node); }, [activateNode, documentSelection, visible]);
  const selector = <><select aria-label="Fly to project map" className="ct-field max-w-[min(18rem,55vw)]" defaultValue="" onChange={(event) => { if (event.target.value) flyTo(event.target.value); }}><option value="">Maps</option>{world.projects.map((project) => <option key={project.file.project_path} value={project.file.project_path} title={displayPath(project.file.project_path)}>{projectName(project.file.project_path)}</option>)}</select>{toolbarView}</>;
  const toolbarExtras = onOpenCartographer && <button type="button" className="ct-bar-pill" onClick={() => onOpenCartographer("refresh")}>Refresh map</button>;
  if (!visible) return <div className="absolute inset-0 min-w-0 overflow-hidden bg-[var(--ct-bg)]" data-testid="essence-map-document-surface"><CanvasToolbar label="Essence map controls" view={toolbarView} extras={toolbarExtras} />{source && <EssenceSourcePanel source={source} onClose={() => setSource(null)} />}</div>;
  return <div className="absolute inset-0 flex min-w-0 flex-col overflow-hidden bg-[var(--ct-bg)]" data-testid="essence-map-canvas" role="region" aria-label={`Workspace essence map for ${workspace.name}`}><div ref={viewport.containerRef} className="relative min-h-0 min-w-0 flex-1 overflow-hidden" data-testid="essence-map-projects" aria-label="Essence map projects" onMouseDown={viewport.handlers.onMouseDown} style={{ cursor: viewport.isDragging ? "grabbing" : "grab", touchAction: "none", background: "radial-gradient(ellipse at center, var(--ct-titlebar-bg) 0%, var(--ct-bg) 100%)" }}><CanvasToolbar label="Essence map controls" view={!loading && !error && hasContent ? selector : toolbarView} onFit={!loading && !error && hasContent ? fitAll : undefined} onZoomIn={!loading && !error && hasContent ? () => zoomBy(ZOOM_STEP) : undefined} onZoomOut={!loading && !error && hasContent ? () => zoomBy(-ZOOM_STEP) : undefined} onRefresh={!loading && !error && hasContent ? onRetry : undefined} refreshTestId="essence-map-refresh" extras={!loading && !error && hasContent ? toolbarExtras : undefined} />{loading && <div className="flex h-full items-center justify-center p-6 text-xs text-[var(--ct-muted-fg)]" role="status">Reading project essence maps…</div>}{!loading && error && <div className="flex h-full flex-col items-center justify-center gap-3 p-6 text-center text-xs text-[var(--ct-red)]" role="alert"><AlertTriangle size={18} aria-hidden="true" /><p>{error}</p>{onRetry && <button type="button" className={UI_PANEL.drawerClose} onClick={onRetry}>Try again</button>}</div>}{!loading && !error && projects.length === 0 && <div className="flex h-full items-center justify-center p-6 text-center text-xs text-[var(--ct-muted-fg)]" data-testid="essence-map-empty">No project trees are attached to this workspace.</div>}{!loading && !error && projects.length > 0 && !hasContent && <EssenceMapEmptyState projects={projects} orchestrator={orchestrator} onAskOrchestrator={onAskOrchestrator} onRetry={onRetry} onCreateMap={onOpenCartographer ? () => onOpenCartographer("create") : undefined} />}{!loading && !error && hasContent && <><div ref={viewport.layerRef} data-testid="essence-map-transform-layer" className="absolute left-0 top-0" style={{ width: world.width, height: world.height, transform: formatLayerTransform(viewport.transform), transformOrigin: "0 0" }}>{world.projects.map((project) => <EssenceMapProjectView key={project.file.project_path} project={project} onActivate={activateNode} />)}</div></>}{source && <EssenceSourcePanel source={source} onClose={() => setSource(null)} />}</div></div>;
}

export function EssenceMapCanvasInner(props: EssenceMapCanvasInnerProps) {
  const [documentTree, setDocumentTree] = useState(false);
  const [cartographerMode, setCartographerMode] = useState<CartographerMapMode | null>(null);
  const [documentSelection, setDocumentSelection] = useState<EssenceMapCanvasInnerProps["documentSelection"]>(null);
  const projects = useMemo(() => props.mapFiles.map(parseEssenceMapProject), [props.mapFiles]);
  const selectDocument = useCallback((project: EssenceMapProject, entry: EssenceDocumentEntry) => setDocumentSelection({ project, entry }), []);
  const { orchestrator, onAskOrchestrator } = props;
  const askOrchestrator = orchestrator && onAskOrchestrator ? () => onAskOrchestrator(orchestrator) : undefined;
  return <div className="absolute inset-0 min-w-0 overflow-hidden">
    <EssenceMapCanvasInnerBase {...props} visible={!documentTree} documentSelection={documentSelection} onOpenCartographer={props.onStartCartographer ? setCartographerMode : undefined} toolbarView={<button type="button" data-testid="essence-map-view-toggle" aria-label={documentTree ? "Switch to graph" : "Switch to document tree"} aria-pressed={documentTree} onClick={() => setDocumentTree((current) => !current)} className="ct-bar-pill">{documentTree ? <Network size={13} aria-hidden="true" /> : <List size={13} aria-hidden="true" />}{documentTree ? "Graph" : "Documents"}</button>} />

    {cartographerMode && props.onStartCartographer && <CartographerMapDialog mode={cartographerMode} providers={props.providers ?? []} loading={props.providersLoading} catalogueError={props.providersError} onCancel={() => setCartographerMode(null)} onStart={props.onStartCartographer} onAskOrchestrator={askOrchestrator} />}
    {!props.loading && !props.error && projects.length > 0 && documentTree && <EssenceDocumentTree projects={projects} onSelect={selectDocument} />}

  </div>;
}

function useWorkspaceEssenceMapData(workspace: Workspace | undefined, tabs: Record<string, Tab>) {
  // Mux hands back fresh tab and workspace objects many times a second while
  // agents run; keying on the paths themselves keeps the fetch effect still.
  const projectPathKey = JSON.stringify(workspace ? projectPathsForWorkspace(workspace, tabs) : []);
  const projectPaths = useMemo(() => JSON.parse(projectPathKey) as string[], [projectPathKey]);
  const [state, setState] = useState<{ status: "loading" | "ready" | "error"; files: EssenceMapFile[]; error: string | null }>({ status: "loading", files: [], error: null });
  useEffect(() => { let active = true; const paths = JSON.parse(projectPathKey) as string[]; if (paths.length === 0) { setState({ status: "ready", files: [], error: null }); return () => { active = false; }; } setState((current) => ({ ...current, status: "loading", error: null })); void fetchWorkspaceEssenceMaps(paths).then((files) => { if (active) setState({ status: "ready", files, error: null }); }, (reason) => { if (active) setState({ status: "error", files: [], error: reason instanceof Error ? reason.message : String(reason) }); }); return () => { active = false; }; }, [projectPathKey]);
  const retry = useCallback(() => { if (projectPaths.length === 0) return; setState((current) => ({ ...current, status: "loading", error: null })); void fetchWorkspaceEssenceMaps(projectPaths).then((files) => setState({ status: "ready", files, error: null }), (reason) => setState({ status: "error", files: [], error: reason instanceof Error ? reason.message : String(reason) })); }, [projectPaths]);
  return { ...state, retry };
}

export function EssenceMapCanvas({ workspaceId, onClose }: EssenceMapCanvasProps) {
  const state = useMuxState();
  const { createOrchestrationPair, selectTabByTab } = useMultiplexer();
  const { providers, loading: providersLoading, error: providersError } = useAgentProviders();
  const workspace = state.workspaces.find((candidate) => candidate.id === workspaceId);
  const { status, files, error, retry } = useWorkspaceEssenceMapData(workspace, state.tabs);
  const orchestrator = workspace ? findWorkspaceOrchestrator(workspace, state.tabs) : null;
  const askOrchestrator = useCallback(async (target: MapOrchestrator) => {
    await askOrchestratorToBuildMap(workspaceId, target, files.map((file) => file.project_path));
    onClose();
  }, [files, onClose, workspaceId]);
  const startMap = async (draft: CartographerMapDraft) => {
    const result = await startCartographerMap({ workspaceId, projectPaths: files.map((file) => file.project_path), draft }, cartographerMapActions(createOrchestrationPair));
    await selectTabByTab(workspaceId, result.tab_id);
    onClose();
  };
  useEffect(() => { if (!workspace) onClose(); }, [onClose, workspace]);
  if (!workspace) return null;
  return <EssenceMapCanvasInner workspace={workspace} mapFiles={files} loading={status === "loading"} error={error} onRetry={retry} sourceReader={fetchEssenceSource} orchestrator={orchestrator} onAskOrchestrator={askOrchestrator} providers={providers} providersLoading={providersLoading} providersError={providersError} onStartCartographer={startMap} />;
}
