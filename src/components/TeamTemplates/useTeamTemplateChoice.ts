import { useCallback, useEffect, useState } from "react";
import { teamTemplatesTransport } from "../../lib/transport/orchestrationRuntime";
import type { TeamTemplatesTransport, TemplatePreview, TemplateRecall, TemplateStartRequest } from "../../lib/transport/teamTemplates";
import {
  buildStartRequest, initialInputs, pickerSeed, startBlocker, startErrorView,
  type PickerSeed, type TemplateSummary,
} from "../../lib/teamTemplates/startModel";
import type { ManagerSeats } from "./TemplateManagers";

type ManagerSlot = import("../../../gen/ManagerSlot").ManagerSlot;

export type TemplateView =
  | { kind: "form" }
  | { kind: "gallery" }
  | { kind: "review"; preview: TemplatePreview; mode: "consent" | "detail" };

export interface TemplateCatalog { templates: TemplateSummary[]; recall: TemplateRecall | null; error: string | null }

const DEFAULT_SUMMARY: TemplateSummary = {
  name: "dev.domios/default", version: "", title: "Default team", provenance: "builtin",
  description: "Orchestrator and Planner; every built-in worker available on demand.",
};

function basename(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}

/** Template choice for the Team form; the default is always available and starts today's pair without `template`. */
export function useTeamTemplateChoice({ enabled, recallQuery, onSeedPair, transport = teamTemplatesTransport }: {
  enabled: boolean;
  recallQuery: string;
  onSeedPair: (seats: Partial<Record<"orchestrator" | "planner", PickerSeed>>) => void;
  transport?: () => TeamTemplatesTransport;
}) {
  const [view, setView] = useState<TemplateView>({ kind: "form" });
  const [catalog, setCatalog] = useState<TemplateCatalog | null>(null);
  const [chosen, setChosen] = useState<TemplatePreview | null>(null);
  const [inputs, setInputs] = useState<Record<string, string>>({});
  const [optOut, setOptOut] = useState<ReadonlySet<string>>(new Set());
  const [seats, setSeats] = useState<ManagerSeats>({});
  const [pending, setPending] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);

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
    setInputs(initialInputs(preview.form.inputs ?? []));
    setOptOut(kept);
    setSeats(managerSeeds);
    setView({ kind: "form" });
  }, [onSeedPair]);

  const openPreview = useCallback(async (template: { name: string; version?: string | null }, detail = false) => {
    setPending(true);
    setNotice(null);
    try {
      const preview = await transport().preview(template);
      setOptOut(new Set());
      if (detail) setView({ kind: "review", preview, mode: "detail" });
      else if (preview.consent) setView({ kind: "review", preview, mode: "consent" });
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
      setView({ kind: "form" });
      return;
    }
    void openPreview(summary);
  }, [defaultSummary.name, openPreview]);

  const importPath = useCallback(async (path: string) => {
    setPending(true);
    setNotice(null);
    try {
      const report = await transport().import(path);
      if (!report.valid || !report.name) {
        const reason = report.diagnostics[0]?.message ?? "there's no team.yaml inside";
        setNotice(`${basename(path)} isn't a team package: ${reason}.`);
        return;
      }
      setCatalog(null);
      await openPreview({ name: report.name, version: report.version });
    } catch (error) {
      setNotice(startErrorView(error).message);
    } finally {
      setPending(false);
    }
  }, [openPreview, transport]);

  const startRequest = useCallback((): TemplateStartRequest | undefined => {
    if (!chosen) return undefined;
    const managerModels = Object.fromEntries(Object.entries(seats).map(([slot, seat]) => [slot,
      { model: `${seat.providerId}:${seat.modelId}`, reasoning_effort: seat.effortId ?? null }]));
    return buildStartRequest({ preview: chosen, inputs, optOut, managerModels });
  }, [chosen, inputs, optOut, seats]);

  return {
    view, catalog, chosen, inputs, optOut, seats, pending, notice, defaultSummary,
    blocker: chosen ? startBlocker(chosen, inputs) : null,
    openGallery: () => { setNotice(null); setView({ kind: "gallery" }); },
    backToForm: () => setView({ kind: "form" }),
    showDetails: () => { if (chosen) setView({ kind: "review", preview: chosen, mode: "detail" }); },
    showSummaryDetails: (summary: TemplateSummary) => { void openPreview(summary, true); },
    choose,
    acceptReviewed: () => {
      if (view.kind !== "review") return;
      if (view.preview === chosen) setView({ kind: "form" });
      else if (view.mode === "detail" && view.preview.consent) setView({ ...view, mode: "consent" });
      else select(view.preview, optOut);
    },
    importPath: (path: string) => { void importPath(path); },
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
