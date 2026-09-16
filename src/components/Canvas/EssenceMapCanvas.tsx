import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { AlertTriangle, RefreshCw, X } from "lucide-react";
import { useMuxState } from "../../store/multiplexer";
import type { Tab, Workspace } from "../../lib/types";
import { UI_PANEL, UI_TEXT } from "../shared/design";
import { MarkdownDescription } from "../Extensions/MarkdownDescription";
import { invoke } from "../../lib/bridge";
import { fetchEssenceSource, fetchWorkspaceEssenceMaps } from "./essenceMapApi";
import { extractEssenceSection } from "./essenceSourceSection";
import { EssenceMapProjectView, projectName } from "./EssenceMapProjectView";
import { layoutEssenceMapWorld, parseEssenceMapProject, projectPathsForWorkspace, type EssenceMapFile, type EssenceMapNode, type EssenceMapProject } from "./essenceMapModel";
import { formatLayerTransform, useCanvasViewport } from "./useCanvasViewport";

interface EssenceMapCanvasProps { workspaceId: string; onClose: () => void; }
interface EssenceMapCanvasInnerProps { workspace: Workspace; mapFiles: EssenceMapFile[]; loading?: boolean; error?: string | null; onRetry?: () => void; sourceReader?: (projectPath: string, sourcePath: string) => Promise<string>; }
interface SourcePanelState { node: EssenceMapNode; projectPath: string; state: "loading" | "ready" | "error"; content?: string; error?: string; }
const DEFAULT_WIDTH = 720;
const ZOOM_STEP = 0.25;

/** Windows extended-length paths carry a \\?\ prefix that must never reach the UI. */
function displayPath(path: string): string { return path.replace(/^\\\\\?\\/, ""); }

function EssenceSourcePanel({ source, onClose }: { source: SourcePanelState; onClose: () => void }) {
  const panelRef = useRef<HTMLElement>(null);
  useLayoutEffect(() => { panelRef.current?.focus(); }, []);
  return <aside ref={panelRef} className="absolute inset-y-0 right-0 z-30 flex w-[min(36rem,100%)] min-w-0 flex-col border-l border-[var(--ct-border)] bg-[var(--ct-titlebar-bg)] p-4 shadow-2xl outline-none" data-testid="essence-source-panel" role="dialog" aria-labelledby="essence-source-panel-title" tabIndex={-1}><div className="flex min-w-0 items-start justify-between gap-3 border-b border-[var(--ct-border)] pb-3"><div className="min-w-0"><p className={UI_TEXT.sectionLabel}>Referenced essence</p><h2 id="essence-source-panel-title" className="truncate text-sm font-semibold text-[var(--ct-fg)]">{source.node.label}</h2><button type="button" data-testid="essence-source-open" className={`${UI_TEXT.detailMuted} block max-w-full truncate text-left hover:text-[var(--ct-accent)] hover:underline focus-visible:outline focus-visible:outline-2 focus-visible:outline-[var(--ct-accent)]`} title={`Open ${displayPath(source.projectPath)}/${source.node.source.path}`} onClick={() => { void invoke("open_folder", { path: `${displayPath(source.projectPath)}/${source.node.source.path}` }); }}>{displayPath(source.projectPath)} · {source.node.source.path} · {source.node.source.anchor}</button><p className="sr-only">Owner vector or essence: {source.node.vector ?? source.node.id}</p></div><button type="button" className={UI_PANEL.drawerClose} aria-label="Close referenced essence" onClick={onClose}><X size={14} aria-hidden="true" /></button></div><div className="min-h-0 flex-1 overflow-auto pt-3">{source.state === "loading" && <div className="flex items-center gap-2 text-xs text-[var(--ct-muted-fg)]" role="status"><RefreshCw size={13} className="animate-spin" aria-hidden="true" />Reading referenced essence…</div>}{source.state === "error" && <p className="text-xs text-[var(--ct-red)]" role="alert">{source.error ?? "Referenced essence could not be read."}</p>}{source.state === "ready" && <MarkdownDescription markdown={extractEssenceSection(source.content ?? "", source.node.source.anchor)} />}</div></aside>;
}

function MapControlPanel({ projects, onFlyTo, onFitAll, onZoomIn, onZoomOut, onRefresh }: { projects: ReturnType<typeof layoutEssenceMapWorld>["projects"]; onFlyTo: (path: string) => void; onFitAll: () => void; onZoomIn: () => void; onZoomOut: () => void; onRefresh?: () => void }) {
  return <div data-essence-map-control className="absolute left-3 top-3 z-20 flex max-w-[calc(100%-1.5rem)] min-w-0 items-center gap-2 rounded-lg border border-[var(--ct-border)] bg-[color-mix(in_srgb,var(--ct-titlebar-bg)_94%,transparent)] p-2 shadow-lg"><label className="sr-only" htmlFor="essence-map-select">Fly to project map</label><select id="essence-map-select" aria-label="Fly to project map" className="min-w-0 max-w-[min(18rem,55vw)] rounded border border-[var(--ct-border)] bg-[var(--ct-bg)] px-2 py-1 text-xs text-[var(--ct-fg)]" defaultValue="" onChange={(event) => { if (event.target.value) onFlyTo(event.target.value); }}><option value="">Maps</option>{projects.map((project) => <option key={project.file.project_path} value={project.file.project_path} title={displayPath(project.file.project_path)}>{projectName(project.file.project_path)}</option>)}</select><button type="button" data-essence-map-control onClick={onFitAll} className="rounded border border-[var(--ct-border)] px-2 py-1 text-xs text-[var(--ct-fg)] hover:border-[var(--ct-accent)] focus-visible:outline focus-visible:outline-2 focus-visible:outline-[var(--ct-accent)]">Fit all</button><button type="button" data-essence-map-control aria-label="Zoom out" onClick={onZoomOut} className="rounded border border-[var(--ct-border)] px-2 py-1 text-xs leading-none text-[var(--ct-fg)] hover:border-[var(--ct-accent)] focus-visible:outline focus-visible:outline-2 focus-visible:outline-[var(--ct-accent)]">&minus;</button><button type="button" data-essence-map-control aria-label="Zoom in" onClick={onZoomIn} className="rounded border border-[var(--ct-border)] px-2 py-1 text-xs leading-none text-[var(--ct-fg)] hover:border-[var(--ct-accent)] focus-visible:outline focus-visible:outline-2 focus-visible:outline-[var(--ct-accent)]">+</button>{onRefresh && <button type="button" data-essence-map-control data-testid="essence-map-refresh" aria-label="Update maps" title="Re-read the essence maps from disk" onClick={onRefresh} className="flex items-center rounded border border-[var(--ct-border)] px-2 py-1 text-xs leading-none text-[var(--ct-fg)] hover:border-[var(--ct-accent)] focus-visible:outline focus-visible:outline-2 focus-visible:outline-[var(--ct-accent)]"><RefreshCw size={12} aria-hidden="true" /></button>}</div>;
}

export function EssenceMapCanvasInner({ workspace, mapFiles, loading = false, error = null, onRetry, sourceReader = fetchEssenceSource }: EssenceMapCanvasInnerProps) {
  const viewportRef = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState({ width: DEFAULT_WIDTH, height: 600 });
  const [source, setSource] = useState<SourcePanelState | null>(null);
  const requestRef = useRef(0);
  const projects = useMemo(() => mapFiles.map(parseEssenceMapProject), [mapFiles]);
  const world = useMemo(() => layoutEssenceMapWorld(projects, size.width), [projects, size.width]);
  const hasContent = projects.some((project) => project.status === "valid" && project.nodes.length > 0);
  const viewport = useCanvasViewport({ width: size.width, height: size.height, minScale: 0.2, maxScale: 2.5, fitPadding: 48, ignoreSelector: "[data-essence-map-control]", wheelBehavior: "zoom" });
  const { fitToView, animateToView, zoomBy } = viewport;
  const fitAll = useCallback(() => fitToView({ x: 0, y: 0, width: world.width, height: world.height }), [fitToView, world.width, world.height]);
  useLayoutEffect(() => { const element = viewportRef.current; if (!element) return; const update = () => { if (element.clientWidth > 0 && element.clientHeight > 0) setSize({ width: element.clientWidth, height: element.clientHeight }); }; update(); if (typeof ResizeObserver === "undefined") return; const observer = new ResizeObserver(update); observer.observe(element); return () => observer.disconnect(); }, []);
  // Fit once per map and viewport size. Keying the effect on fitAll re-fit on every
  // render, which reset the camera a frame after each zoom or pan.
  const fittedKey = useRef<string | null>(null);
  useEffect(() => {
    if (world.projects.length === 0 || size.width <= 0 || size.height <= 0) return;
    // Keyed on the maps alone, never the size: the view mounts at a default width and
    // the observer reports the real one a moment later, so a size-keyed fit re-fires
    // after the viewer has already zoomed or panned and throws their camera away.
    const key = world.projects.map((project) => project.file.project_path).join("|");
    if (fittedKey.current === key) return;
    fittedKey.current = key;
    fitAll();
  }, [fitAll, size.height, size.width, world.projects]);
  useEffect(() => { if (source && !projects.some((project) => project.nodes.some((node) => node.id === source.node.id))) setSource(null); }, [projects, source]);
  const flyTo = useCallback((path: string) => { const bounds = world.projectBounds.get(path); if (bounds) animateToView(bounds); }, [animateToView, world.projectBounds]);
  const activateNode = useCallback((project: EssenceMapProject, node: EssenceMapNode) => { const request = requestRef.current + 1; requestRef.current = request; setSource({ node, projectPath: project.file.project_path, state: "loading" }); void sourceReader(project.file.project_path, node.source.path).then((content) => { if (requestRef.current === request) setSource({ node, projectPath: project.file.project_path, state: "ready", content }); }, (reason) => { if (requestRef.current === request) setSource({ node, projectPath: project.file.project_path, state: "error", error: reason instanceof Error ? reason.message : String(reason) }); }); }, [sourceReader]);
  return <div className="absolute inset-0 flex min-w-0 flex-col overflow-hidden bg-[var(--ct-bg)]" data-testid="essence-map-canvas" role="region" aria-label={`Workspace essence map for ${workspace.name}`}><div ref={viewportRef} className="relative min-h-0 min-w-0 flex-1 overflow-hidden" data-testid="essence-map-projects" aria-label="Essence map projects" onMouseDown={viewport.handlers.onMouseDown} style={{ cursor: viewport.isDragging ? "grabbing" : "grab", touchAction: "none", background: "radial-gradient(ellipse at center, var(--ct-titlebar-bg) 0%, var(--ct-bg) 100%)" }}>{loading && <div className="flex h-full items-center justify-center p-6 text-xs text-[var(--ct-muted-fg)]" role="status">Reading project essence maps…</div>}{!loading && error && <div className="flex h-full flex-col items-center justify-center gap-3 p-6 text-center text-xs text-[var(--ct-red)]" role="alert"><AlertTriangle size={18} aria-hidden="true" /><p>{error}</p>{onRetry && <button type="button" className={UI_PANEL.drawerClose} onClick={onRetry}>Try again</button>}</div>}{!loading && !error && projects.length === 0 && <div className="flex h-full items-center justify-center p-6 text-center text-xs text-[var(--ct-muted-fg)]" data-testid="essence-map-empty">No project trees are attached to this workspace.</div>}{!loading && !error && projects.length > 0 && !hasContent && <div className="absolute left-1/2 top-20 z-10 -translate-x-1/2 px-5 text-center text-xs text-[var(--ct-muted-fg)]" data-testid="essence-map-no-nodes">No source-backed essence nodes are available. Empty, malformed, oversized, or missing maps are shown below without inventing nodes.</div>}{!loading && !error && projects.length > 0 && <><MapControlPanel projects={world.projects} onFlyTo={flyTo} onFitAll={fitAll} onZoomIn={() => zoomBy(ZOOM_STEP)} onZoomOut={() => zoomBy(-ZOOM_STEP)} onRefresh={onRetry} /><div ref={viewport.layerRef} className="absolute left-0 top-0" style={{ width: world.width, height: world.height, transform: formatLayerTransform(viewport.transform), transformOrigin: "0 0" }}>{world.projects.map((project) => <EssenceMapProjectView key={project.file.project_path} project={project} onActivate={activateNode} />)}</div></>}{source && <EssenceSourcePanel source={source} onClose={() => setSource(null)} />}</div></div>;
}

function useWorkspaceEssenceMapData(workspace: Workspace | undefined, tabs: Record<string, Tab>) {
  const projectPaths = useMemo(() => workspace ? projectPathsForWorkspace(workspace, tabs) : [], [tabs, workspace]);
  const [state, setState] = useState<{ status: "loading" | "ready" | "error"; files: EssenceMapFile[]; error: string | null }>({ status: "loading", files: [], error: null });
  useEffect(() => { let active = true; if (projectPaths.length === 0) { setState({ status: "ready", files: [], error: null }); return () => { active = false; }; } setState((current) => ({ ...current, status: "loading", error: null })); void fetchWorkspaceEssenceMaps(projectPaths).then((files) => { if (active) setState({ status: "ready", files, error: null }); }, (reason) => { if (active) setState({ status: "error", files: [], error: reason instanceof Error ? reason.message : String(reason) }); }); return () => { active = false; }; }, [projectPaths]);
  const retry = useCallback(() => { if (projectPaths.length === 0) return; setState((current) => ({ ...current, status: "loading", error: null })); void fetchWorkspaceEssenceMaps(projectPaths).then((files) => setState({ status: "ready", files, error: null }), (reason) => setState({ status: "error", files: [], error: reason instanceof Error ? reason.message : String(reason) })); }, [projectPaths]);
  return { ...state, retry };
}

export function EssenceMapCanvas({ workspaceId, onClose }: EssenceMapCanvasProps) { const state = useMuxState(); const workspace = state.workspaces.find((candidate) => candidate.id === workspaceId); const { status, files, error, retry } = useWorkspaceEssenceMapData(workspace, state.tabs); useEffect(() => { if (!workspace) onClose(); }, [onClose, workspace]); if (!workspace) return null; return <EssenceMapCanvasInner workspace={workspace} mapFiles={files} loading={status === "loading"} error={error} onRetry={retry} sourceReader={fetchEssenceSource} />; }
