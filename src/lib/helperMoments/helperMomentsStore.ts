import { adjacentSuggestion } from "./adjacency";
import { featureInUse, UNKNOWN_FACTS, type HelperFacts } from "./helperBlocks";
import {
  admitSuggestion,
  EMPTY_MEMORY,
  parseHelperMemory,
  recordOutcome,
  recordShown,
  type AdmitVerdict,
  type HelperMemory,
  type HelperOutcome,
  type HelperSuggestion,
} from "./helperRules";
import { EMPTY_TRACKER, observeSignal, type HelperSignal, type HelperTabKind, type MomentTracker } from "./momentRegistry";

export interface ActiveHelperSuggestion extends HelperSuggestion {
  accepted: boolean;
}

export interface HelperMomentsSnapshot {
  active: ActiveHelperSuggestion | null;
  memory: HelperMemory;
}

export interface HelperMomentsPorts {
  load(): unknown;
  save(memory: HelperMemory): void;
  tabKind(tabId: string): HelperTabKind;
  /** Resolves whether the tab's workspace already runs a Blueprint; null when unknown. */
  blueprintsInUse(tabId: string): Promise<boolean | null>;
}

/** The helper's single slot: moments and adjacent offers compete for it under the helper rules. */
export function createHelperMomentsStore(ports: HelperMomentsPorts) {
  let memory: HelperMemory = (() => {
    try {
      return parseHelperMemory(ports.load());
    } catch {
      return EMPTY_MEMORY;
    }
  })();
  let tracker: MomentTracker = EMPTY_TRACKER;
  let facts: HelperFacts = UNKNOWN_FACTS;
  let snapshot: HelperMomentsSnapshot = { active: null, memory };
  let answerPending = false;
  const listeners = new Set<() => void>();

  const publish = (active: ActiveHelperSuggestion | null, nextMemory = memory) => {
    if (nextMemory !== memory) {
      memory = nextMemory;
      ports.save(memory);
    }
    snapshot = { active, memory };
    for (const listener of listeners) listener();
  };

  const offer = (candidate: HelperSuggestion, candidateFacts: HelperFacts = facts): AdmitVerdict => {
    const verdict = admitSuggestion(memory, snapshot.active, candidate, candidateFacts);
    if (verdict.admit) publish({ ...candidate, accepted: false });
    return verdict;
  };

  const settle = (outcome: HelperOutcome) => {
    const active = snapshot.active;
    if (!active) return;
    if (outcome === "accepted") {
      if (!active.accepted) publish({ ...active, accepted: true }, recordOutcome(memory, active, "accepted"));
      return;
    }
    publish(null, active.accepted && outcome === "dismissed" ? memory : recordOutcome(memory, active, outcome));
  };

  return {
    subscribe(listener: () => void) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    getSnapshot: () => snapshot,
    facts: () => facts,
    setFacts(patch: Partial<HelperFacts>) {
      facts = { ...facts, ...patch };
      const active = snapshot.active;
      if (active && !active.accepted && featureInUse(active.block, facts) === true) publish(null);
    },
    async observe(signal: HelperSignal): Promise<AdmitVerdict | null> {
      if (signal.kind === "user-message") answerPending = true;
      const result = observeSignal(tracker, signal, facts, ports.tabKind);
      tracker = result.tracker;
      const hit = result.hit;
      if (!hit) return null;
      const candidate: HelperSuggestion = { momentId: hit.momentId, block: hit.block, benefit: hit.benefit };
      if (hit.block.kind === "guide" && hit.block.sectionId === "blueprints" && hit.tabId) {
        const inUse = await ports.blueprintsInUse(hit.tabId).catch(() => null);
        return offer(candidate, { ...facts, blueprintsInUse: inUse });
      }
      return offer(candidate);
    },
    /** Offers one strong-edge neighbour for the answer to the user's latest message, considered once. */
    offerAdjacent(answerText: string): AdmitVerdict | null {
      if (!answerPending) return null;
      answerPending = false;
      const found = adjacentSuggestion(answerText, facts);
      return found ? offer({ momentId: "adjacent_capability", ...found }) : null;
    },
    /** Counts the first render of the active suggestion; it never shows again after this. */
    markShown() {
      const active = snapshot.active;
      const next = active ? recordShown(memory, active) : memory;
      if (active && next !== memory) publish(active, next);
    },
    /** Drops the active suggestion without counting, e.g. when the agent's own reply already shows that block. */
    discard() {
      if (snapshot.active && !snapshot.active.accepted) publish(null);
    },
    accept: () => settle("accepted"),
    dismiss: () => settle("dismissed"),
    mute: () => settle("muted"),
  };
}

export type HelperMomentsStore = ReturnType<typeof createHelperMomentsStore>;
