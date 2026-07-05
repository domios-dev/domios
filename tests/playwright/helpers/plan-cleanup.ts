/**
 * Plan-junk cleanup for creation/matrix suites.
 *
 * Closing a workspace does NOT cascade to its plans, so any suite that spawns
 * orchestration pairs (or otherwise creates plans) leaks them unless it deletes
 * them explicitly — the recurring "matrix junk" (bare "Orchestration", empty
 * "04:56" timestamp plans). Cleaning up by TITLE is the trap: the very bug that
 * mis-titles a plan is what makes a title-keyed sweep miss it. This registry
 * keys on the plan-id set instead: snapshot at suite start, sweep everything
 * created since, assert the set returned to baseline so a leak FAILS the suite.
 */
import { ApiClient, type PlanSummary } from "./api-client";

const JUNK_TITLE = /^(orchestration|\d{1,2}:\d{2})$/i;

/** Bare 'Orchestration', any 'orchmatrix*', a HH:MM name, or empty — matrix junk. */
export function isJunkPlanTitle(title: string | undefined | null): boolean {
  const t = (title ?? "").trim();
  return t.length === 0 || JUNK_TITLE.test(t) || t.toLowerCase().includes("orchmatrix");
}

export class PlanRegistry {
  private baseline = new Set<string>();
  private tracked = new Set<string>();
  private _api?: ApiClient;

  constructor(api?: ApiClient) {
    this._api = api;
  }

  // Lazy — a describe-body `new PlanRegistry()` must not read the dev token
  // file at collection time (dev may not be up yet).
  private get api(): ApiClient {
    return (this._api ??= new ApiClient());
  }

  private list(): Promise<PlanSummary[]> {
    return this.api.plans().catch(() => [] as PlanSummary[]);
  }

  /** Capture the pre-existing plan set. Call once at suite start. */
  async snapshot(): Promise<void> {
    this.baseline = new Set((await this.list()).map((p) => p.plan_id));
  }

  /** Belt for a plan whose id is known but might vanish from the list. */
  track(id: string | undefined | null): void {
    if (id) this.tracked.add(id);
  }

  /** Plans that exist now but were absent from the baseline snapshot. */
  async created(): Promise<PlanSummary[]> {
    return (await this.list()).filter((p) => !this.baseline.has(p.plan_id));
  }

  /** Delete every plan created since snapshot + every explicitly tracked id.
   *  Idempotent — safe from afterEach AND afterAll. */
  async sweep(): Promise<void> {
    const ids = new Set(this.tracked);
    for (const p of await this.created()) ids.add(p.plan_id);
    for (const id of ids) await this.api.deletePlan(id).catch(() => {});
  }

  /** Junk-titled plans in the current full list (should be none post-sweep). */
  async junk(): Promise<PlanSummary[]> {
    return (await this.list()).filter((p) => isJunkPlanTitle(p.title));
  }
}
