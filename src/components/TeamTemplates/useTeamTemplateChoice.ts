import { useCallback, useEffect, useState } from "react";
import { teamTemplatesTransport } from "../../lib/transport/orchestrationRuntime";
import type {
  TeamTemplatesTransport, TemplatePeerStartAccepted, TemplatePeerStartRequest, TemplatePreview, TemplateRecall, TemplateStartRequest,
} from "../../lib/transport/teamTemplates";
import { initialSecretBindings } from "../../lib/teamTemplates/exchangeModel";
import { takePendingTemplatePreview } from "../../lib/teamTemplates/pendingPreview";
import { listVaultSecretNames } from "../../lib/teamTemplates/vaultSecrets";
import { listHostPeers, type HostPeer } from "../../lib/teamTemplates/hostPeers";
import {
  buildStartRequest, initialInputs, pickerSeed, startBlocker, startErrorView,
  type PickerSeed, type TemplateSummary,
} from "../../lib/teamTemplates/startModel";
import type { ManagerSeats } from "./TemplateManagers";

type ManagerSlot = import("../../../gen/ManagerSlot").ManagerSlot;
type TemplateUpdateDiff = import("../../../gen/TemplateUpdateDiff").TemplateUpdateDiff;

export type TemplateView =
  | { kind: "form" }
  | { kind: "gallery" }
  | { kind: "review"; preview: TemplatePreview; mode: "consent" | "detail"; back: "form" | "gallery" };

export interface TemplateCatalog { templates: TemplateSummary[]; recall: TemplateRecall | null; error: string | null }

const DEFAULT_SUMMARY: TemplateSummary = {
  name: "dev.domios/default", version: "", title: "Default team", provenance: "builtin", verified: true,
  description: "Orchestrator and Planner; every built-in worker available on demand.",
};

function basename(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}

/** Template choice for the Team form; the default is always available and starts today's pair without `template`. */
export function useTeamTemplateChoice({ enabled, recallQuery, onSeedPair, transport = teamTemplatesTransport, vaultNames = listVaultSecretNames, hostPeers = listHostPeers }: {
  enabled: boolean;
  recallQuery: string;
  onSeedPair: (seats: Partial<Record<"orchestrator" | "planner", PickerSeed>>) => void;
  transport?: () => TeamTemplatesTransport;
  vaultNames?: () => Promise<string[]>;
  hostPeers?: () => Promise<HostPeer[]>;
}) {
  const [view, setView] = useState<TemplateView>({ kind: "form" });
  const [catalog, setCatalog] = useState<TemplateCatalog | null>(null);
  const [chosen, setChosen] = useState<TemplatePreview | null>(null);
  const [inputs, setInputs] = useState<Record<string, string>>({});
  const [optOut, setOptOut] = useState<ReadonlySet<string>>(new Set());
  const [seats, setSeats] = useState<ManagerSeats>({});
  const [pending, setPending] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
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
      if (picker.slot === "orchestrator" || picker.slot === "planner") pairSeeds[picker.slot] = seed;
      else managerSeeds[picker.slot] = seed;
    }
    onSeedPair(pairSeeds);
    setChosen(preview);
    setSecrets(initialSecretBindings(preview.asks ?? [], vault ?? []));
    setInputs(initialInputs(preview.form.inputs ?? []));
    setOptOut(kept);
    setSeats(managerSeeds);
    setPlacements({});
    setRemotePeer(null);
    setView({ kind: "form" });
  }, [onSeedPair, vault]);

  const openPreview = useCallback(async (template: { name: string; version?: string | null }, detail = false, diff: TemplateUpdateDiff | null = null) => {
    setPending(true);
    setNotice(null);
    setUpdate(diff);
    try {
      const preview = await transport().preview(template);
      setOptOut(new Set());
      if (detail) setView({ kind: "review", preview, mode: "detail", back: "gallery" });
      else if (preview.consent) setView({ kind: "review", preview, mode: "consent", back: "gallery" });
      else select(preview, new Set());
    } catch (error) {
      setNotice(startErrorView(error).message);
    } finally {
      setPending(false);
    }
  }, [select, transport]);

  const choose = useCallback((summary: TemplateSummary) => {
    if (summary.name === defaultSummary.name) {
      setChosen(null);
      setPlacements({});
      setRemotePeer(null);
      setView({ kind: "form" });
      return;
    }
    void openPreview(summary);
  }, [defaultSummary.name, openPreview]);

  const importPath = useCallback(async (path: string) => {
    setPending(true);
    setNotice(null);
    try {
      const imported = await transport().import(path);
      setCatalog(null);
      await openPreview({ name: imported.template.name, version: imported.template.version }, false, imported.update ?? null);
    } catch (error) {
      const failure = startErrorView(error);
      const reason = (failure.reasons[0] ?? failure.message).replace(/\.$/, "");
      setNotice(`${basename(path)} isn't a team package: ${reason}.`);
    } finally {
      setPending(false);
    }
  }, [openPreview, transport]);

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
    if (request) void openPreview(request.template, false, request.update);
  }, [enabled, openPreview]);

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

  // The default starts as today's pair; moving any role or the whole team to a peer needs the template start.
  const placed = remotePeer !== null || Object.values(placements).some(Boolean);
  const active = chosen ?? (placed ? defaultPreview : null);
  const startRequest = useCallback((): TemplateStartRequest | undefined => {
    if (!active) return undefined;
    const managerModels = Object.fromEntries(Object.entries(seats).map(([slot, seat]) => [slot,
      { model: `${seat.providerId}:${seat.modelId}`, reasoning_effort: seat.effortId ?? null }]));
    return buildStartRequest({ preview: active, inputs, optOut, managerModels, secretBindings: secrets, rolePlacements: remotePeer ? {} : placements });
  }, [active, inputs, optOut, seats, secrets, placements, remotePeer]);

  return {
    view, catalog, chosen, inputs, optOut, seats, pending, notice, defaultSummary, defaultPreview, secrets, vault, update,
    placements, remotePeer, peers,
    /** True when Start goes through the template start rather than today's default pair. */
    usesTemplate: active !== null,
    blocker: active ? startBlocker(active, inputs, secrets, remotePeer) : null,
    openGallery: () => { setNotice(null); setView({ kind: "gallery" }); },
    backToForm: () => setView({ kind: "form" }),
    showDetails: () => {
      const shown = chosen ?? defaultPreview;
      if (shown) setView({ kind: "review", preview: shown, mode: "detail", back: "form" });
    },
    showSummaryDetails: (summary: TemplateSummary) => { void openPreview(summary, true); },
    choose,
    backFromReview: () => setView(view.kind === "review" && view.back === "form" ? { kind: "form" } : { kind: "gallery" }),
    acceptReviewed: () => {
      if (view.kind !== "review") return;
      if (view.preview === chosen) setView({ kind: "form" });
      else if (view.preview.form.template.name === defaultSummary.name) choose(defaultSummary);
      else if (view.mode === "detail" && view.preview.consent) setView({ ...view, mode: "consent" });
      else select(view.preview, optOut);
    },
    importPath: (path: string) => { void importPath(path); },
    remove: (summary: TemplateSummary) => { void remove(summary); },
    setSecret: (name: string, entry: string) => setSecrets((current) => ({ ...current, [name]: entry })),
    wantPeers: () => setPeersWanted(true),
    /** Asks the chosen peer to start this Team; its owner answers there. */
    startOnPeer: (start: TemplatePeerStartRequest["start"]): Promise<TemplatePeerStartAccepted> =>
      transport().peerStart({ peer: remotePeer ?? "", start }),
    setPlacement: (role: string, peer: string) => setPlacements((current) => ({ ...current, [role]: peer })),
    setRemotePeer,
    setNotice,
    setInput: (key: string, value: string) => setInputs((current) => ({ ...current, [key]: value })),
    setKept: (id: string, kept: boolean) => setOptOut((current) => {
      const next = new Set(current);
      if (kept) next.delete(id); else next.add(id);
      return next;
    }),
    setSeat: (slot: ManagerSlot, seat: PickerSeed) => setSeats((current) => ({ ...current, [slot]: seat })),
    startRequest,
  };
}

export type TeamTemplateChoice = ReturnType<typeof useTeamTemplateChoice>;
