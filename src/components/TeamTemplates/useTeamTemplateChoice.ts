import { useTemplateCatalogRefusals } from "./useTemplateCatalogRefusals";
import { useCallback, useEffect, useState } from "react";
import { teamTemplatesTransport } from "../../lib/transport/orchestrationRuntime";
import type {
  TeamTemplatesTransport, TemplatePeerStartAccepted, TemplatePeerStartRequest, TemplatePreview, TemplateRecall, TemplateStartRequest,
} from "../../lib/transport/teamTemplates";
import { initialSecretBindings } from "../../lib/teamTemplates/exchangeModel";
import { takePendingTemplatePreview } from "../../lib/teamTemplates/pendingPreview";
import { listVaultSecretNames } from "../../lib/teamTemplates/vaultSecrets";
import { listHostPeers, type HostPeer } from "../../lib/teamTemplates/hostPeers";
import { webUpload } from "../../lib/webApi";
import {
  buildStartRequest, initialInputs, pickerSeed, startBlocker, startErrorView, startRefusalView, templateSource, templateTitle, withKept,
  type PickerSeed, type StartErrorView, type TemplateSummary,
} from "../../lib/teamTemplates/startModel";
import type { ManagerSeats } from "./TemplateManagers";

/** A manager role name: a built-in manager or one of the package's own. */
type ManagerSlot = string;
type TemplateUpdateDiff = import("../../../gen/TemplateUpdateDiff").TemplateUpdateDiff;

export type TemplateView =
  | { kind: "form" }
  | { kind: "gallery" }
  | { kind: "review"; preview: TemplatePreview; mode: "consent" | "detail"; back: "form" | "gallery" };

/** A template the host refused to start: its package identity, the title its card shows, and the typed refusal. */
export interface BlockedTemplate { name: string; version: string | null; title: string; refusal: StartErrorView }

export interface TemplateCatalog { templates: TemplateSummary[]; recall: TemplateRecall | null; error: string | null }

const DEFAULT_SUMMARY: TemplateSummary = {
  name: "dev.domios/default", version: "", title: "Default team", provenance: "builtin", verified: true, workers: 4,
  description: "Orchestrator and Planner; every built-in worker available on demand.",
};

function basename(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}

/** Only an unverified package passes through the review before its form; Built-in and Mine go straight to it, still carrying their consent decision. */
function needsReview(preview: TemplatePreview): boolean {
  return preview.consent != null && templateSource(preview.form.template).unverified;
}

/** Template choice for the Team form; the default is always available and starts today's pair without `template`. */
export function useTeamTemplateChoice({ enabled, recallQuery, onSeedPair, transport = teamTemplatesTransport, vaultNames = listVaultSecretNames, hostPeers = listHostPeers, upload = webUpload }: {
  enabled: boolean;
  recallQuery: string;
  onSeedPair: (seats: Partial<Record<"orchestrator" | "planner", PickerSeed>>) => void;
  transport?: () => TeamTemplatesTransport;
  vaultNames?: () => Promise<string[]>;
  hostPeers?: () => Promise<HostPeer[]>;
  upload?: (file: Blob, name: string) => Promise<{ path: string }>;
}) {
  const [view, setView] = useState<TemplateView>({ kind: "form" });
  const [catalog, setCatalog] = useState<TemplateCatalog | null>(null);
  const [chosen, setChosen] = useState<TemplatePreview | null>(null);
  const [touched, setTouched] = useState(false);
  const [inputs, setInputs] = useState<Record<string, string>>({});
  const [optOut, setOptOut] = useState<ReadonlySet<string>>(new Set());
  const [seats, setSeats] = useState<ManagerSeats>({});
  const [pending, setPending] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  /** A template the host refused to start, with the typed refusal's reason. */
  const [blocked, setBlocked] = useState<BlockedTemplate | null>(null);
  const [secrets, setSecrets] = useState<Record<string, string>>({});
  const [vault, setVault] = useState<string[] | null>(null);
  const [update, setUpdate] = useState<TemplateUpdateDiff | null>(null);
  const [placements, setPlacements] = useState<Record<string, string>>({});
  const [remotePeer, setRemotePeer] = useState<string | null>(null);
  const [peers, setPeers] = useState<HostPeer[] | null>(null);
  const [peersWanted, setPeersWanted] = useState(false);

  useEffect(() => {
    if (!enabled || catalog) return;
    const controller = new AbortController();
    void Promise.allSettled([
      Promise.resolve().then(() => transport().list({ signal: controller.signal })),
      Promise.resolve().then(() => transport().recall(recallQuery, { signal: controller.signal })),
    ])
      .then(([list, recall]) => {
        if (controller.signal.aborted) return;
        setCatalog({
          templates: list.status === "fulfilled" ? list.value.templates : [],
          recall: recall.status === "fulfilled" ? recall.value : null,
          error: list.status === "rejected" ? startErrorView(list.reason).message : null,
        });
      });
    return () => controller.abort();
  }, [enabled, catalog, recallQuery, transport]);

  const defaultSummary = catalog?.recall?.fallback ?? DEFAULT_SUMMARY;
  const catalogStatuses = useTemplateCatalogRefusals(enabled && view.kind === "gallery", catalog, defaultSummary, transport);
  const [defaultPreview, setDefaultPreview] = useState<TemplatePreview | null>(null);

  // The default form shows the whole default team; its start still goes through today's pair unless the owner moves roles.
  useEffect(() => {
    if (!enabled || defaultPreview) return;
    const controller = new AbortController();
    void Promise.resolve()
      .then(() => transport().preview({ name: DEFAULT_SUMMARY.name }, { signal: controller.signal }))
      // Without the preview the default form keeps today's pair rows, which still start the default team.
      .then((preview) => { if (!controller.signal.aborted) setDefaultPreview(preview); }, () => undefined);
    return () => controller.abort();
  }, [enabled, defaultPreview, transport]);

  const select = useCallback((preview: TemplatePreview, kept: ReadonlySet<string>) => {
    const pairSeeds: Partial<Record<"orchestrator" | "planner", PickerSeed>> = {};
    const managerSeeds: ManagerSeats = {};
    for (const picker of preview.form.pickers) {
      const seed = pickerSeed(picker.decision);
      if (!seed) continue;
      if (picker.role === "orchestrator" || picker.role === "planner") pairSeeds[picker.role] = seed;
      else managerSeeds[picker.role] = seed;
    }
    onSeedPair(pairSeeds);
    setTouched(false);
    setChosen(preview);
    setSecrets(initialSecretBindings(preview.asks ?? [], vault ?? []));
    setInputs(initialInputs(preview.form.inputs ?? []));
    setOptOut(kept);
    setSeats(managerSeeds);
    setPlacements({});
    setRemotePeer(null);
    setView({ kind: "form" });
  }, [onSeedPair, vault]);

  const openPreview = useCallback(async (template: { name: string; version?: string | null; title?: string | null }, diff: TemplateUpdateDiff | null = null,
    title = templateTitle(template)) => {
    setPending(true);
    setNotice(null);
    setBlocked(null);
    setUpdate(diff);
    try {
      const preview = await transport().preview(template);
      setOptOut(new Set());
      if (needsReview(preview)) setView({ kind: "review", preview, mode: "consent", back: "gallery" });
      else select(preview, new Set());
    } catch (error) {
      const refusal = startRefusalView(error);
      if (refusal) setBlocked({ name: template.name, version: template.version ?? null, title, refusal });
      else setNotice(startErrorView(error).message);
    } finally {
      setPending(false);
    }
  }, [select, transport]);

  const choose = useCallback((summary: TemplateSummary) => {
    setBlocked(null);
    if (summary.name === defaultSummary.name) {
      setTouched(false);
      setChosen(null);
      setSeats({});
      setPlacements({});
      setRemotePeer(null);
      setView({ kind: "form" });
      return;
    }
    void openPreview(summary);
  }, [defaultSummary.name, openPreview]);

  const importPath = useCallback(async (path: string, shownName = basename(path)) => {
    setPending(true);
    setNotice(null);
    try {
      const imported = await transport().import(path);
      setCatalog(null);
      await openPreview({ name: imported.template.name, version: imported.template.version }, imported.update ?? null, templateTitle(imported.template));
    } catch (error) {
      const failure = startErrorView(error);
      const reason = (failure.reasons[0] ?? failure.message).replace(/\.$/, "");
      setNotice(`${shownName} isn't a team package: ${reason}.`);
    } finally {
      setPending(false);
    }
  }, [openPreview, transport]);

  /** A browser has files, not paths: the file is uploaded to the daemon first, then imported like a dropped path. */
  const importUpload = useCallback(async (file: Blob, name: string) => {
    setPending(true);
    setNotice(null);
    let uploaded: string;
    try {
      uploaded = (await upload(file, name)).path;
    } catch (error) {
      setNotice(`${name} couldn't be read: ${startErrorView(error).message.replace(/\.$/, "")}.`);
      setPending(false);
      return;
    }
    await importPath(uploaded, name);
  }, [importPath, upload]);

  const remove = useCallback(async (summary: TemplateSummary) => {
    setPending(true);
    setNotice(null);
    try {
      await transport().delete({ name: summary.name, version: summary.version });
      if (chosen && chosen.form.template.name === summary.name && chosen.form.template.version === summary.version) setChosen(null);
      setCatalog(null);
    } catch (error) {
      setNotice(startErrorView(error).message);
    } finally {
      setPending(false);
    }
  }, [chosen, transport]);

  useEffect(() => {
    if (!enabled) return;
    const request = takePendingTemplatePreview();
    if (!request) return;
    if ("file" in request) void importPath(request.file);
    else void openPreview(request.template, request.update);
  }, [enabled, openPreview, importPath]);

  useEffect(() => {
    if (!peersWanted || peers) return;
    let live = true;
    void hostPeers().then((list) => { if (live) setPeers(list); }, () => { if (live) setPeers([]); });
    return () => { live = false; };
  }, [peersWanted, peers, hostPeers]);

  const needsVault = Boolean(chosen?.asks?.some((ask) => ask.kind === "secret"));
  useEffect(() => {
    if (!needsVault || vault) return;
    let live = true;
    void vaultNames().then((names) => {
      if (!live) return;
      setVault(names);
      setSecrets((current) => ({ ...initialSecretBindings(chosen?.asks ?? [], names), ...current }));
    }, () => { if (live) setVault([]); });
    return () => { live = false; };
  }, [needsVault, vault, vaultNames, chosen]);

  // The default starts as today's pair; a picked later-manager model or a placement needs the template start.
  const customized = remotePeer !== null || Object.values(placements).some(Boolean) || Object.keys(seats).length > 0;
  const active = chosen ?? (customized ? defaultPreview : null);
  const startRequest = useCallback((): TemplateStartRequest | undefined => {
    if (!active) return undefined;
    const managerModels = Object.fromEntries(Object.entries(seats).flatMap(([role, seat]) => seat ? [[role,
      { model: `${seat.providerId}:${seat.modelId}`, reasoning_effort: seat.effortId ?? null }] as const] : []));
    return buildStartRequest({ preview: active, inputs, optOut, managerModels, secretBindings: secrets, rolePlacements: remotePeer ? {} : placements });
  }, [active, inputs, optOut, seats, secrets, placements, remotePeer]);

  return {
    view, catalog, chosen, touched, inputs, optOut, seats, pending, notice, blocked, catalogStatuses, defaultSummary, defaultPreview, secrets, vault, update,
    placements, remotePeer, peers,
    /** True when Start goes through the template start rather than today's default pair. */
    usesTemplate: active !== null,
    blocker: active ? startBlocker(active, inputs, secrets, remotePeer) : null,
    openGallery: () => { setNotice(null); setView({ kind: "gallery" }); },
    backToForm: () => setView({ kind: "form" }),
    choose,
    backFromReview: () => setView(view.kind === "review" && view.back === "form" ? { kind: "form" } : { kind: "gallery" }),
    acceptReviewed: () => {
      if (view.kind !== "review") return;
      if (view.preview === chosen) setView({ kind: "form" });
      else if (view.preview.form.template.name === defaultSummary.name) choose(defaultSummary);
      else if (view.mode === "detail" && needsReview(view.preview)) setView({ ...view, mode: "consent" });
      else select(view.preview, optOut);
    },
    importPath: (path: string) => { void importPath(path); },
    importUpload: (file: Blob, name: string) => { void importUpload(file, name); },
    remove: (summary: TemplateSummary) => { void remove(summary); },
    setSecret: (name: string, entry: string) => { setTouched(true); setSecrets((current) => ({ ...current, [name]: entry })); },
    wantPeers: () => setPeersWanted(true),
    /** Asks the chosen peer to start this Team; its owner answers there. */
    startOnPeer: (start: TemplatePeerStartRequest["start"]): Promise<TemplatePeerStartAccepted> =>
      transport().peerStart({ peer: remotePeer ?? "", start }),
    setPlacement: (role: string, peer: string) => { setTouched(true); setPlacements((current) => ({ ...current, [role]: peer })); },
    setRemotePeer: (peer: string | null) => { setTouched(true); setRemotePeer(peer); },
    setNotice,
    setInput: (key: string, value: string) => { setTouched(true); setInputs((current) => ({ ...current, [key]: value })); },
    setKept: (ids: readonly string[], kept: boolean) => { setTouched(true); setOptOut((current) => withKept(current, ids, kept)); },
    setSeat: (slot: ManagerSlot, seat: PickerSeed) => { setTouched(true); setSeats((current) => ({ ...current, [slot]: seat })); },
    clearSeat: (slot: ManagerSlot) => setSeats((current) => {
      const next = { ...current };
      delete next[slot];
      return next;
    }),
    startRequest,
  };
}

export type TeamTemplateChoice = ReturnType<typeof useTeamTemplateChoice>;
