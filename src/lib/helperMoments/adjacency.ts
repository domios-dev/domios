import { blockSource, blocksInText, featureInUse, type HelperBlock, type HelperFacts } from "./helperBlocks";

/** Which essence-map capabilities each Guide section explains. */
export const SECTION_CAPABILITIES: Readonly<Record<string, readonly string[]>> = {
  workspaces: ["workspaces-and-tabs"],
  agents: ["teams-and-roles", "task-fitted-personality", "dispatch-and-reports"],
  plans: ["goals-specs-plans-and-tasks", "review-and-decisions", "reminders-milestones-and-pause"],
  memory: ["memory-and-skills", "memory-hygiene", "memory-decision-rerank"],
  "supporter-agent": ["supporter-agent"],
  mesh: ["pairing-and-trust", "remote-work"],
  messengers: ["webhook-receivers", "text-hooks", "service-registry"],
  phone: ["telegram-bridge", "voice"],
  "messenger-plugins": ["plugins-and-channels"],
  blueprints: ["sentinels", "scheduling"],
};

export const PLUGIN_CAPABILITIES: readonly string[] = ["plugins-and-channels"];

/** Direct `feeds` edges of essences/map.jsonl between the capabilities above; a test keeps each one real. */
export const STRONG_CAPABILITY_EDGES: readonly (readonly [string, string])[] = [
  ["task-fitted-personality", "workspaces-and-tabs"],
  ["goals-specs-plans-and-tasks", "dispatch-and-reports"],
  ["dispatch-and-reports", "review-and-decisions"],
  ["review-and-decisions", "dispatch-and-reports"],
  ["remote-work", "dispatch-and-reports"],
  ["telegram-bridge", "text-hooks"],
  ["webhook-receivers", "voice"],
  ["supporter-agent", "memory-decision-rerank"],
  ["supporter-agent", "scheduling"],
  ["supporter-agent", "sentinels"],
  ["plugins-and-channels", "webhook-receivers"],
  ["plugins-and-channels", "voice"],
];

/** Sections the helper may offer, in preference order; each has a usage fact and a one-sentence benefit. */
export const ADJACENT_SECTIONS: readonly { sectionId: string; benefit: string }[] = [
  { sectionId: "phone", benefit: "Related: answer your agents from your phone through a Telegram bot." },
  { sectionId: "messenger-plugins", benefit: "Related: let agents read and send from your own Telegram and Teams accounts." },
  { sectionId: "blueprints", benefit: "Related: a Blueprint watches for a change and acts on it without you." },
];

function capabilitiesOf(block: HelperBlock): readonly string[] {
  return block.kind === "plugin" ? PLUGIN_CAPABILITIES : SECTION_CAPABILITIES[block.sectionId] ?? [];
}

/** Guide sections joined to the answered blocks by a strong edge in either direction. */
export function strongNeighbourSections(blocks: readonly HelperBlock[]): Set<string> {
  const answered = new Set(blocks.flatMap(capabilitiesOf));
  const neighbours = new Set<string>();
  for (const [from, to] of STRONG_CAPABILITY_EDGES) {
    const other = answered.has(from) ? to : answered.has(to) ? from : null;
    if (!other || answered.has(other)) continue;
    for (const [sectionId, capabilities] of Object.entries(SECTION_CAPABILITIES)) {
      if (capabilities.includes(other)) neighbours.add(sectionId);
    }
  }
  return neighbours;
}

/** One related, provably unused capability block for an answer that already showed capability blocks. */
export function adjacentSuggestion(answerText: string, facts: HelperFacts): { block: HelperBlock; benefit: string } | null {
  const blocks = blocksInText(answerText);
  if (!blocks.length) return null;
  const shown = new Set(blocks.map(blockSource));
  const neighbours = strongNeighbourSections(blocks);
  for (const { sectionId, benefit } of ADJACENT_SECTIONS) {
    const block: HelperBlock = { kind: "guide", sectionId };
    if (neighbours.has(sectionId) && !shown.has(blockSource(block)) && featureInUse(block, facts) === false) return { block, benefit };
  }
  return null;
}
