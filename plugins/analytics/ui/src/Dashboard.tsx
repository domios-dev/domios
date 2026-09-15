import { useCallback, useEffect, useMemo, useState } from "react";
import type { AnalyticsGroupBy } from "@codeterm/plugin-sdk";
import { BoardPanel, BoardPanelSection } from "../../../../src/components/Canvas/missionControl/BoardPanel";
import { DeferredLoader } from "../../../../src/components/shared/DeferredLoader";
import { closeView } from "./bridge";
import { AllowanceCard } from "./components/AllowanceCard";
import { Banner } from "./components/Banner";
import { Breakdown } from "./components/Breakdown";
import { ChartCard } from "./components/ChartCard";
import { FilterControls, type OpenFilter } from "./components/Filters";
import { Header } from "./components/Header";
import { TotalsTiles } from "./components/Tiles";
import type { Measure } from "./model/chart";
import { groupLabels, toneMap } from "./model/keys";
import { activeFilterCount, NO_FILTERS, type FilterState } from "./model/query";
import { rangeCaption, type RangePreset } from "./model/range";
import { dashboardView } from "./model/view";
import { useAnalytics } from "./useAnalytics";
import { useNarrow } from "./useNarrow";

export function Dashboard() {
  const narrow = useNarrow();
  const [preset, setPreset] = useState<RangePreset>("7d");
  const [groupBy, setGroupBy] = useState<AnalyticsGroupBy>("provider_model");
  const [measure, setMeasure] = useState<Measure>("new_input");
  const [filters, setFilters] = useState<FilterState>(NO_FILTERS);
  const [openFilter, setOpenFilter] = useState<OpenFilter>(null);
  const data = useAnalytics({ preset, groupBy, filters });
  const filterCount = activeFilterCount(filters);
  const view = dashboardView(data.outcome, filterCount > 0);
  const snapshot = "snapshot" in view ? view.snapshot : null;
  const catalogGroups = data.catalog[groupBy]?.tokens.groups;
  const tones = useMemo(() => toneMap(catalogGroups ?? []), [catalogGroups]);
  const labels = useMemo(() => groupLabels(catalogGroups ?? []), [catalogGroups]);

  const closeFilter = useCallback(() => {
    setOpenFilter((open) => {
      const trigger = open === "sheet" ? "analytics-filters-open" : open ? `analytics-filter-${open}` : null;
      if (trigger) document.querySelector<HTMLElement>(`[data-testid="${trigger}"]`)?.focus();
      return null;
    });
  }, []);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      if (openFilter !== null) closeFilter();
      else closeView();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [closeFilter, openFilter]);

  const retry = (
    <button type="button" className="mc-board-chip" onClick={data.refresh}>
      Try again
    </button>
  );
  const clear = (
    <button type="button" data-testid="analytics-clear-filters" className="mc-board-more" onClick={() => setFilters(NO_FILTERS)}>
      Clear filters
    </button>
  );
  const allowance = snapshot && <AllowanceCard snapshot={snapshot} catalog={data.catalog} filters={filters} />;

  return (
    <main
      className="mc-board analytics"
      data-testid="analytics-dashboard"
      data-layout={narrow ? "narrow" : "wide"}
      data-view={view.kind}
      aria-labelledby="analytics-title"
      aria-busy={data.refreshing}
    >
      <Header snapshot={snapshot} filterCount={filterCount} refreshing={data.refreshing} onRefresh={data.refresh} />
      <FilterControls
        preset={preset}
        onPreset={setPreset}
        filters={filters}
        onFilters={setFilters}
        catalog={data.catalog}
        rangeText={rangeCaption(data.range)}
        narrow={narrow}
        open={openFilter}
        onOpen={(next) => (next === null ? closeFilter() : setOpenFilter(next))}
        turnsInView={snapshot?.tokens.totals?.turns ?? null}
      />

      {view.kind === "loading" && (
        <DeferredLoader state="present" role="status" aria-label="Loading analytics" className="analytics-loading">
          <div className="analytics-skeleton" />
          <div className="analytics-skeleton" />
          <div className="analytics-skeleton analytics-skeleton-tall" />
        </DeferredLoader>
      )}

      {view.kind === "host_unavailable" && (
        <Banner testId="analytics-host-unavailable" tone="error" title="Analytics didn't answer" action={retry}>
          The host couldn't read this machine's analytics. Figures are hidden rather than shown as zero.
        </Banner>
      )}

      {view.kind === "refused" && (
        <Banner testId="analytics-refused" tone="error" title="This query was refused" action={filterCount > 0 ? clear : retry}>
          {view.message}
        </Banner>
      )}

      {view.kind === "ledger_unavailable" && (
        <>
          <Banner testId="analytics-ledger-unavailable" tone="error" title="Can't read the KPI ledger" action={retry}>
            {view.reason ?? "The host gave no reason"}. Figures are hidden rather than shown as zero.
          </Banner>
          <TotalsTiles snapshot={view.snapshot} unavailable />
          {allowance}
        </>
      )}

      {view.kind === "nothing_recorded" && (
        <>
          <BoardPanel testId="analytics-nothing-recorded" title="Nothing recorded yet">
            <BoardPanelSection>
              <p className="mc-board-note">
                No turns are recorded on this machine yet. Recording is on; totals start with the first agent turn that reports token counts.
              </p>
            </BoardPanelSection>
          </BoardPanel>
          {allowance}
        </>
      )}

      {view.kind === "no_match" && (
        <>
          <BoardPanel testId="analytics-no-match" title="No matches">
            <BoardPanelSection>
              <p className="mc-board-note">No turns match these filters in this range, and recording is on. Widen the range or clear the filters.</p>
              {clear}
            </BoardPanelSection>
          </BoardPanel>
          {allowance}
        </>
      )}

      {view.kind === "data" && (
        <>
          {view.recordingOff && (
            <Banner testId="analytics-recording-off" tone="warn" title="Recording is off">
              Turns and tabs aren't recorded while {view.snapshot.recording.enable_setting ?? "KPI recording"} is off, so every bucket reads
              as not recorded. Turn it on in Settings.
            </Banner>
          )}
          <TotalsTiles snapshot={view.snapshot} unavailable={false} />
          {view.snapshot.money.state === "not_recorded" && (
            <p className="mc-board-note">
              <b>Tokens, not money.</b> CodeTerm records token counts per turn and applies no prices.
            </p>
          )}
          <ChartCard
            snapshot={view.snapshot}
            tones={tones}
            labels={labels}
            measure={measure}
            onMeasure={setMeasure}
            nowMs={data.nowMs}
            narrow={narrow}
          />
          <div className="analytics-columns">
            <Breakdown snapshot={view.snapshot} tones={tones} groupBy={groupBy} onGroupBy={setGroupBy} narrow={narrow} />
            {allowance}
          </div>
        </>
      )}

      <p className="mc-board-caption analytics-footer">
        Analytics adds up what was recorded. Plan decisions and events stay on each team's Board.
      </p>
    </main>
  );
}
