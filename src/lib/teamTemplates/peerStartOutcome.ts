import type { Tab } from "../types";
import { startErrorView, type StartErrorView } from "./startModel";

export type TemplatePeerStartOutcome = import("../../../gen/TemplatePeerStartOutcome").TemplatePeerStartOutcome;

/** The origin event that carries every peer start outcome. */
export const TEMPLATE_PEER_OUTCOME_EVENT = "team-template-peer-outcome";

/** The started Team's managers to show here, Planner first so the Orchestrator ends up selected. */
export function managersToAttach(outcome: TemplatePeerStartOutcome, peerTabs: readonly Tab[]): Tab[] {
  if (outcome.state !== "started") return [];
  return [outcome.planner_tab_id, outcome.orchestrator_tab_id]
    .map((id) => (id ? peerTabs.find((tab) => tab.id === id) : undefined))
    .filter((tab): tab is Tab => tab !== undefined);
}

/** What the start form says when a peer start did not start a Team. */
export function peerOutcomeError(outcome: TemplatePeerStartOutcome): StartErrorView {
  const peer = outcome.peer_name;
  const plain = (message: string): StartErrorView => ({ message, reasons: [], choices: [] });
  switch (outcome.state) {
    case "started":
      return plain(`The Team started on ${peer}.`);
    case "refused":
      return outcome.refusal
        ? startErrorView(JSON.stringify(outcome.refusal))
        : plain(`${peer} refused the start: ${outcome.error ?? "no reason given"}`);
    case "failed":
      return plain(`${peer} could not start the Team: ${outcome.error ?? "no reason given"}`);
    case "declined":
      return plain(`The owner of ${peer} declined the start.`);
    default:
      return plain(`${peer} gave no answer about the start (${outcome.state.replace("_", " ")}).`);
  }
}
